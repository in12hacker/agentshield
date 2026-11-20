//! Main's native event wiring. Source waiting stays in the adapter; matching stays in rule modules.
use crate::{dispatch::MainIngress, messages::driver::DriverPort};
use as_types::{driver::DriverEvent, main_ports::*};
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) async fn run(
    mut events: mpsc::Receiver<DriverEvent>,
    port: DriverPort,
    ingress: MainIngress,
    modules: MainModules,
    tree: Option<std::sync::Arc<as_process_tree::ProcessTree>>,
) -> Result<(), ModuleError> {
    // This producer belongs to the actual source lifetime, not external ingress clones.
    let _producer = ingress
        .driver_message_producer()
        .ok_or(ModuleError::Stopped)?;
    let mut loss = port.callback_loss()?;
    if *loss.borrow_and_update() != 0 {
        modules.fast_facts.source_gap();
    }
    loop {
        let event = tokio::select! {biased;
            changed=loss.changed()=>{changed.map_err(|_|ModuleError::Unavailable)?;loss.borrow_and_update();modules.fast_facts.source_gap();continue;},
            event=events.recv()=>match event{Some(event)=>event,None=>break}
        };
        match event {
            DriverEvent::Observation(event) => ingress.native_observation(event).await?,
            DriverEvent::FileClose(record) => ingress.driver_file_close(record).await,
            DriverEvent::RequestReturn(fact) => {
                modules
                    .messages
                    .consume_native_fact(NativeFact::RequestReturn(fact))
                    .await?
            }
            DriverEvent::RequestHandoff(fact) => {
                modules
                    .messages
                    .consume_native_fact(NativeFact::RequestHandoff(fact))
                    .await?
            }
            DriverEvent::FutexSelection(fact) => {
                modules
                    .messages
                    .consume_native_fact(NativeFact::FutexSelection(fact))
                    .await?
            }
            DriverEvent::DirectHits(batch) => ingress.native_hits(batch).await?,
            DriverEvent::DirectResult {
                mut event,
                mut choice,
                outcome,
                termination,
            } => {
                if let Some(tree) = &tree
                    && !event.bind_host_boot(tree.boot_id())
                {
                    return Err(ModuleError::InvalidInput);
                }
                if let Some(hit) = &mut choice.hit
                    && let RuleSourceVersion::RulesDocument(version) = hit.source
                {
                    match port
                        .lease(version)
                        .and_then(|lease| lease.resolve_hit_feedback(hit))
                    {
                        Ok(resolved) => *hit = resolved,
                        Err(error) => {
                            tracing::warn!(%error,"original driver feedback text unavailable")
                        }
                    }
                }
                match modules.fast_facts.prepare(&event) {
                    Ok(facts) => event.process.fill_missing(facts),
                    Err(error) => tracing::warn!(%error,"direct driver result facts unavailable"),
                }
                let event = std::sync::Arc::new(event);
                if let Some(hit) = &choice.hit {
                    if hit.log {
                        ingress.driver_log(RuleLogRequest {
                            decision_id: None,
                            event: event.clone(),
                            hit: hit.clone(),
                            decision: RuleLogDecision::Resolved {
                                decision: choice.decision,
                                reason: choice.reason,
                            },
                            result: Ok(DispatchOutcome::Driver {
                                outcome,
                                termination,
                            }),
                        });
                    }
                } else {
                    return Err(ModuleError::InvalidInput);
                }
                modules
                    .fast_facts
                    .finish_event(as_types::process_tree::EventKey::from(event.as_ref()));
                ingress
                    .driver_notice(MainMessage::Completed(Box::new(MainNotice {
                        decision_id: None,
                        event,
                        choice: Some(choice),
                        result: Ok(DispatchOutcome::Driver {
                            outcome,
                            termination,
                        }),
                    })))
                    .await?;
            }
            DriverEvent::Status(status) => {
                ingress
                    .driver_notice(MainMessage::DriverStatus(status))
                    .await?
            }
            DriverEvent::PersistentRetired { generation } => port.retired(generation)?,
            DriverEvent::TransportUnavailable => {
                modules.fast_facts.source_gap();
                ingress
                    .driver_notice(MainMessage::DriverUnavailable)
                    .await?;
                ingress.close();
            }
            DriverEvent::FactsBarrier { marker, processed } => {
                if let Err(error) = ingress.fact_barrier().await {
                    if ingress.is_closed() {
                        drop(processed);
                        continue;
                    }
                    return Err(error);
                }
                if let Some(tree) = &tree {
                    tree.fence(&marker);
                }
                let _ = processed.send(());
            }
        }
    }
    if ingress.is_closed() {
        Ok(())
    } else {
        Err(ModuleError::Unavailable)
    }
}

/// Consume only original retained operations; auxiliary SQL/log waits never occupy this receiver.
pub(crate) async fn controlled(
    mut controlled: mpsc::Receiver<DispatchInput>,
    port: DriverPort,
    ingress: MainIngress,
    modules: MainModules,
    tree: Option<std::sync::Arc<as_process_tree::ProcessTree>>,
) -> Result<(), ModuleError> {
    let _producer = ingress
        .driver_message_producer()
        .ok_or(ModuleError::Stopped)?;
    while let Some(mut input) = controlled.recv().await {
        if let Some(tree) = &tree
            && !Arc::make_mut(&mut input.event).bind_host_boot(tree.boot_id())
        {
            return Err(ModuleError::InvalidInput);
        }
        let Some(operation) = input.operation.clone() else {
            return Err(ModuleError::InvalidInput);
        };
        if operation.try_result().is_some() {
            ingress
                .native_work(input, Err(ModuleError::Stopped))
                .await?;
            continue;
        }
        let lease = operation
            .policy_version()
            .ok_or(ModuleError::InvalidInput)
            .and_then(|version| port.lease(version));
        if let Err(error) = &lease {
            if operation.try_result().is_some() {
                ingress
                    .native_work(input, Err(ModuleError::Stopped))
                    .await?;
                continue;
            }
            modules.fast_facts.source_gap();
            tracing::error!(%error,"native selected policy has no retained exact generation");
            if let Err(failure) = port
                .request(as_types::driver_control::DriverCommand::DeclareFailure)
                .await
            {
                tracing::warn!(%failure,"native source failure acknowledgement unavailable");
            }
        }
        ingress.native_work(input, lease).await?;
    }
    if ingress.is_closed() {
        Ok(())
    } else {
        Err(ModuleError::Unavailable)
    }
}
