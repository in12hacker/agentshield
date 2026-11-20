//! Shared Unix/WS admission for original Main commands and private credential operations.
use super::{
    INFLIGHT, Output, PendingReplies, PrivateCommand, PrivateData, PrivateReply, PrivateRequest,
    TransportError, module_code, ordinary, received_reply, rejected, rejected_diagnostic,
    reply_unavailable,
};
use as_main::messages::{MainMessages, connections::Connection, protocol::*};
use as_types::{flow::CredentialError, main_ports::ModuleError};
use futures::FutureExt;
use serde_json::value::RawValue;
use std::sync::Arc;

pub(super) enum Request<'a> {
    Message(&'a RawValue),
    Private(&'a RawValue),
}

pub(super) fn admit(
    request: Request<'_>,
    messages: &Arc<MainMessages>,
    connection: &Connection,
    pending: &mut PendingReplies,
) -> Result<Option<Output>, TransportError> {
    let raw = match request {
        Request::Message(raw) => {
            let command =
                serde_json::from_str(raw.get()).map_err(|_| TransportError::InvalidFrame)?;
            return Ok(submit(command, messages, connection, pending));
        }
        Request::Private(raw) => raw,
    };
    let admin = match messages.credential_admin(connection) {
        Ok(admin) => admin,
        Err(error) => {
            return Ok(Some(rejected_diagnostic(
                "private_not_authorized",
                module_code(error),
            )));
        }
    };
    let PrivateRequest { id, command } =
        serde_json::from_str(raw.get()).map_err(|_| TransportError::InvalidFrame)?;
    if pending.len() >= INFLIGHT {
        return Ok(Some(Output::Private(PrivateReply {
            id,
            result: Err(CredentialError::Busy),
        })));
    }
    pending.push(
        async move {
            let result = match command {
                PrivateCommand::EnsureCa => admin.ensure_ca().await.map(PrivateData::Ca),
                PrivateCommand::PublicCa { ca } => admin.public_ca(ca).map(PrivateData::Ca),
                PrivateCommand::ImportSecret { value } => {
                    admin.import_secret(value.0).await.map(PrivateData::Saved)
                }
                PrivateCommand::ReplaceSecret {
                    secret,
                    expected_revision,
                    value,
                } => admin
                    .replace_secret(secret, expected_revision, value.0)
                    .await
                    .map(PrivateData::Saved),
                PrivateCommand::IssuePhantom {
                    binding,
                    expected_config,
                } => admin
                    .issue_phantom(binding, expected_config)
                    .await
                    .map(PrivateData::Issued),
                PrivateCommand::RevokePhantom { token } => {
                    admin.revoke_phantom(token).await.map(PrivateData::Revoked)
                }
            };
            Output::Private(PrivateReply { id, result })
        }
        .boxed(),
    );
    Ok(None)
}

fn submit(
    command: Down,
    messages: &Arc<MainMessages>,
    connection: &Connection,
    pending: &mut PendingReplies,
) -> Option<Output> {
    let id = command.command_id();
    if let Some(id) = id
        && pending.len() >= INFLIGHT
    {
        return Some(ordinary(rejected(id, ModuleError::Busy), None));
    }
    match messages.submit(connection, command) {
        Ok(Some(reply)) => pending.push(
            async move {
                match reply.await {
                    Ok(reply) => received_reply(reply),
                    Err(_) => reply_unavailable(id, "backend_reply_unavailable"),
                }
            }
            .boxed(),
        ),
        Ok(None) => {}
        Err(error) => {
            return Some(match id {
                Some(id) => ordinary(rejected(id, error), None),
                None => rejected_diagnostic("request_rejected", module_code(error)),
            });
        }
    }
    None
}
