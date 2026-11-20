//! Validate the one persisted non-secret egress policy before any owner adopts it.
use super::ConfigError;
use as_types::{configuration::ConfigDocument, flow::*};
use std::collections::HashSet;

pub(super) fn validate(document: &ConfigDocument) -> Result<(), ConfigError> {
    let invalid = || ConfigError::Invalid("invalid controlled TLS settings".into());
    let routes = document
        .flow
        .as_ref()
        .map(|f| f.provider_routes.as_slice())
        .unwrap_or(&[]);
    let mut route_ids = HashSet::new();
    let mut selectors = HashSet::new();
    for route in routes {
        let origin = HttpsOrigin::parse(&route.origin).map_err(|_| invalid())?;
        if origin.ascii_host.parse::<std::net::IpAddr>().is_ok() {
            return Err(ConfigError::Unsupported(
                "controlled TLS routes require DNS SNI",
            ));
        }
        if !route_ids.insert(route.id)
            || !selectors.insert((origin, route.method.clone(), route.path.clone()))
            || http::Method::from_bytes(route.method.as_bytes()).is_err()
            || route.method != route.method.to_ascii_uppercase()
            || !route.path.starts_with('/')
            || route.path.contains(['?', '#'])
            || route.path.parse::<http::uri::PathAndQuery>().is_err()
        {
            return Err(invalid());
        }
        if let Some(field) = &route.session {
            let valid = match field {
                SessionField::Header { name } => {
                    name == &name.to_ascii_lowercase()
                        && http::header::HeaderName::from_bytes(name.as_bytes()).is_ok()
                        && !matches!(
                            name.as_str(),
                            "authorization"
                                | "x-api-key"
                                | "proxy-authorization"
                                | "cookie"
                                | "set-cookie"
                        )
                }
                SessionField::JsonPointer { pointer } => {
                    pointer.starts_with('/')
                        && pointer
                            .split('~')
                            .skip(1)
                            .all(|tail| matches!(tail.as_bytes().first(), Some(b'0' | b'1')))
                }
            };
            if !valid {
                return Err(invalid());
            }
        }
    }
    let mut bindings = HashSet::new();
    for binding in &document.credentials.bindings {
        if !bindings.insert(binding.id)
            || binding.subjects.is_empty()
            || binding.routes.is_empty()
            || binding.routes.iter().any(|r| !route_ids.contains(r))
            || binding.routes.iter().collect::<HashSet<_>>().len() != binding.routes.len()
        {
            return Err(invalid());
        }
        for subject in &binding.subjects {
            if matches!(subject, CredentialSubject::Process(p) if p.host_tgid == 0) {
                return Err(invalid());
            }
        }
    }
    let Some(flow) = &document.flow else {
        return Ok(());
    };
    let d = &flow.decode_limits;
    if d.max_connection_buffer_bytes == 0
        || d.max_message_bytes == 0
        || d.max_message_bytes > d.max_connection_buffer_bytes
        || d.max_active_exchanges == 0
        || d.max_json_depth == 0
        || d.max_json_depth > 128
        || flow.event_queue_bytes == 0
        || flow.event_queue_bytes > u32::MAX as u64
    {
        return Err(invalid());
    }
    if flow.mode != FlowMode::MediateTls {
        return Err(ConfigError::Unsupported("passive TLS capture"));
    }
    if flow.enabled && (routes.is_empty() || flow.egress.is_none()) {
        return Err(invalid());
    }
    if let Some(e) = &flow.egress
        && (e.max_connections == 0
            || e.max_streams_per_connection == 0
            || u64::from(e.max_connections) > tokio::sync::Semaphore::MAX_PERMITS as u64
            || u64::from(e.max_streams_per_connection) > tokio::sync::Semaphore::MAX_PERMITS as u64)
    {
        return Err(invalid());
    }
    Ok(())
}
