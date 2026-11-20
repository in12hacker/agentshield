//! Main composes configured owners; TLS and ingress retain their own implementations.
use crate::assembly::AssemblyError;
use as_types::{
    driver::{TlsIngressInput, TlsIngressSession},
    flow::*,
    main_ports::{MessageModule, ModuleError},
};
use std::sync::Arc;

pub(crate) struct FlowRuntime {
    module: as_flow::FlowModule,
    ingress: Arc<dyn TlsIngressSession>,
    chat_consumer: tokio::task::JoinHandle<Result<(), ModuleError>>,
}
impl FlowRuntime {
    pub(crate) fn ingress_port(&self) -> Arc<dyn TlsIngressSession> {
        self.ingress.clone()
    }
    pub(crate) fn status_port(&self) -> Arc<dyn FlowStatusPort> {
        self.module.status_port()
    }
    pub(crate) async fn start(
        snapshot: Arc<as_runtime_config::ConfigSnapshot>,
        credentials: as_credentials::CredentialModule,
        driver: crate::messages::driver::DriverPort,
        messages: Arc<dyn MessageModule>,
        boot_id: [u8; 16],
    ) -> Result<Option<Self>, AssemblyError> {
        let Some(settings) = snapshot.document().flow.as_ref().filter(|f| f.enabled) else {
            return Ok(None);
        };
        let egress = settings.egress.as_ref().ok_or(FlowError::InvalidInput)?;
        let (tx, rx) = tokio::sync::mpsc::channel(egress.max_connections as usize);
        let (output, events) = ChatSender::channel(
            u32::try_from(settings.event_queue_bytes).map_err(|_| FlowError::InvalidInput)?,
        );
        let (module, destinations) = as_flow::FlowModule::initialize(as_flow::FlowInit {
            output,
            feedback_capacity: snapshot.document().queues.messages,
            settings: Arc::new(settings.clone()),
            config_version: snapshot.version(),
            ingress_rx: rx,
            credentials: credentials.use_port(),
            certificates: credentials.certificate_port(),
        })
        .await?;
        let feedback = module.feedback_port();
        let chat_consumer =
            tokio::spawn(async move { messages.consume_chat(events, Some(feedback)).await });
        let input = TlsIngressInput {
            boot_id,
            scope: settings.scope.clone(),
            destinations,
            config_version: snapshot.version(),
            max_connections: egress.max_connections,
        };
        let ingress = match driver.start_tls_ingress(input, tx).await {
            Ok(ingress) => ingress,
            Err(error) => {
                if let Err(failure) = module.stop().await {
                    tracing::error!(%failure, "Flow stop failed during TLS startup retirement");
                }
                match chat_consumer.await {
                    Ok(Ok(())) => {}
                    Ok(Err(failure)) => {
                        tracing::error!(%failure, "chat consumer failed during TLS startup retirement")
                    }
                    Err(failure) => {
                        tracing::error!(%failure, "chat consumer join failed during TLS startup retirement")
                    }
                }
                return Err(error.into());
            }
        };
        Ok(Some(Self {
            module,
            ingress,
            chat_consumer,
        }))
    }
    pub(crate) async fn stop(self) -> std::io::Result<()> {
        let ingress = self.ingress.stop_accepting().await;
        let module = self.module.stop().await;
        let consumed = self.chat_consumer.await;
        match consumed {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(std::io::Error::other(error)),
            Err(error) => return Err(std::io::Error::other(error)),
        }
        ingress.map_err(|error| {
            crate::runtime::retirement_error("ingress_stop", std::io::Error::other(error))
        })?;
        module.map_err(|error| {
            crate::runtime::retirement_error("flow_stop", std::io::Error::other(error))
        })?;
        Ok(())
    }
}
