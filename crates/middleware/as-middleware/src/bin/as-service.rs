//! AgentShield host composing the real Linux driver and authenticated middleware transports.
use as_main::assembly::{MainAssembly, MainComponents};
use as_middleware::{business_log::BusinessLog, media::Media, transport};
use as_types::configuration::Principal;
use futures::{FutureExt, future::BoxFuture};
use std::{io, sync::Arc, time::Duration};
type LocalSession = BoxFuture<'static, Result<(), transport::TransportError>>;

async fn poll_session(session: &mut Option<LocalSession>) -> Result<(), transport::TransportError> {
    match session {
        Some(work) => work.await,
        None => std::future::pending().await,
    }
}
async fn poll_diagnostic(pending: &mut Option<BoxFuture<'static, ()>>) {
    match pending {
        Some(work) => work.await,
        None => std::future::pending().await,
    }
}
async fn drain_websockets(
    clients: &mut tokio::task::JoinSet<Result<(), transport::TransportError>>,
) -> io::Result<()> {
    let mut result = Ok(());
    while let Some(completed) = clients.join_next().await {
        match completed {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                as_types::diagnostics::record_stderr(format!("AS_WS_SESSION_CLOSED code={error}"))
                    .await;
                if matches!(error, transport::TransportError::ChatDrain) {
                    result = result.and(Err(io::Error::other(error)));
                }
            }
            Err(error) => result = result.and(Err(io::Error::other(error))),
        }
    }
    result
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = run().await;
    let diagnostics = as_types::diagnostics::flush_stdio().await;
    result?;
    diagnostics?;
    Ok(())
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().len() != 1 {
        return Err(io::Error::other("as-service takes no arguments").into());
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut assembly = MainAssembly::initialize().await?;
    let mut prompts = assembly
        .take_prompts()
        .ok_or_else(|| io::Error::other("prompt receiver missing"))?;
    let business =
        BusinessLog::initialize(assembly.context().paths(), assembly.log_storage().clone()).await?;
    let media = Arc::new(Media::with_group(
        business.clone(),
        assembly.configuration().startup_group_observation(),
    ));
    let (_stop, stopped) = tokio::sync::broadcast::channel(1);
    let starting = assembly
        .start(
            Arc::new(as_driver_adapter::LinuxDriverAdapter),
            MainComponents {
                messages: media.clone(),
            },
            stopped,
        )
        .await;
    let mut threads = match starting {
        Ok(threads) => threads,
        Err(error) => {
            business.close().await?;
            return Err(error.into());
        }
    };
    let mut stop_listener = threads.bind_stop_listener()?;
    let messages = assembly.messages(&threads);
    let peer_uid = match assembly
        .configuration()
        .snapshot()
        .document()
        .middleware
        .peer
    {
        Principal::UnixUid { uid } => uid,
        _ => {
            let retired = threads.shutdown().await;
            let closed = business.close().await;
            retired?;
            closed?;
            return Err(io::Error::other("Linux requires Unix peer policy").into());
        }
    };
    let endpoint =
        match transport::Endpoint::bind(assembly.context().paths().middleware_endpoint(), peer_uid)
        {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let retired = threads.shutdown().await;
                let closed = business.close().await;
                retired?;
                closed?;
                return Err(error.into());
            }
        };
    let websocket = async {
        let snapshot = assembly.configuration().snapshot();
        match &snapshot.document().middleware.websocket {
            Some(ws) => transport::WebSocketEndpoint::bind(ws.listen)
                .await
                .map(Some),
            None => Ok(None),
        }
    }
    .await;
    let websocket = match websocket {
        Ok(listener) => listener,
        Err(error) => {
            let retired = threads.shutdown().await;
            let closed = business.close().await;
            retired?;
            closed?;
            return Err(error.into());
        }
    };
    let websocket_capacity = assembly
        .configuration()
        .snapshot()
        .document()
        .queues
        .messages
        .get();
    let mut clients = tokio::task::JoinSet::new();
    let mut diagnostic: Option<BoxFuture<'static, ()>> =
        Some(as_types::diagnostics::record_stdout("AS_SCRIPTED_SERVICE_READY".into()).boxed());
    let mut session: Option<LocalSession> = None;
    let mut diagnostics = tokio::time::interval(Duration::from_secs(1));
    let mut last_rejections = None;
    // These are finite protocol error categories without peer IDs or payloads.
    // Count repetitions while the one current diagnostic is backpressured.
    let mut session_errors = std::collections::BTreeMap::<String, u64>::new();
    let mut last_retention_deferred = 0;
    let mut stop_request = None;
    let result:Result<(),Box<dyn std::error::Error>>=async {
        loop{tokio::select!{biased;
            _=terminate.recv()=>break,
            _=interrupt.recv()=>break,
            request=&mut stop_listener=>{
                let request=request.map_err(|_|io::Error::other("host stop listener ended without a request"))?;
                stop_request=Some(request);
                break;
            },
            _=poll_diagnostic(&mut diagnostic),if diagnostic.is_some()=>{diagnostic=None;},
            _=business.write_failed()=>return Err(io::Error::other("business database write failed").into()),
            Some(prompt)=prompts.recv()=>media.prompt(prompt)?,
            result=poll_session(&mut session),if session.is_some()=>{
                if let Err(error)=result {
                    let count=session_errors.entry(format!("AS_SCRIPTED_SESSION_CLOSED code={error}")).or_default();
                    *count=count.saturating_add(1);
                }
                media.close_peer()?;session=None;
            },
            _=diagnostics.tick(),if diagnostic.is_none()=>{
                if let Some((code,count))=session_errors.pop_first(){
                    diagnostic=Some(as_types::diagnostics::record_stderr(format!("{code} count={count}")).boxed());
                    continue;
                }
                let deferred=assembly.log_storage().deferred_maintenance();
                if deferred!=last_retention_deferred{
                    last_retention_deferred=deferred;
                    diagnostic=Some(as_types::diagnostics::record_stderr(format!("AS_LOG_STORAGE_MAINTENANCE_DEFERRED count={deferred}")).boxed());
                    continue;
                }
                if let (Some(notices),Some(logs))=(threads.rejected_notices(),threads.rejected_logs()){
                    let counts=(notices,logs);
                    if last_rejections!=Some(counts){
                        diagnostic=Some(as_types::diagnostics::record_stderr(format!("AS_SCRIPTED_DELIVERY_REJECTIONS notices={notices} logs={logs}")).boxed());
                        last_rejections=Some(counts);
                    }
                }
            },
            Some(completed)=clients.join_next(),if !clients.is_empty()=>{
                match completed {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        let count=session_errors.entry(format!("AS_WS_SESSION_CLOSED code={error}")).or_default();
                        *count=count.saturating_add(1);
                    },
                    Err(error) => return Err(error.into()),
                }
            },
            accepted=async {
                match &websocket {
                    Some(listener) => listener.accept().await,
                    None => std::future::pending().await,
                }
            },if websocket.is_some() && clients.len() < websocket_capacity=>{
                let stream=accepted?;
                let snapshot=assembly.configuration().snapshot();
                if let Some(ws)=&snapshot.document().middleware.websocket {
                    let origin=ws.origin.clone();
                    let media=media.clone();
                    let messages=messages.clone();
                    clients.spawn(async move { media.serve_websocket(stream, origin, messages).await });
                }
            },
            accepted=endpoint.accept(),if session.is_none()=>{
                let socket=accepted?;
                match messages.connect(socket){
                    Ok(connection)=>{let(peer,work)=transport::session(messages.clone(),connection,business.clone());media.set_peer(Some(peer))?;session=Some(work.boxed());},
                    Err(_)=>{
                        let count=session_errors.entry("AS_SCRIPTED_AUTH_REJECTED".into()).or_default();
                        *count=count.saturating_add(1);
                    },
                }
            },
        }}Ok(())
    }.await;
    drop(websocket);
    let peers = media.begin_chat_drain();
    let retirement = threads.shutdown_draining_content();
    tokio::pin!(retirement);

    let mut session_closed = Ok(());
    if let Some(mut request) = stop_request {
        let outcome = tokio::select! {biased;
            written=&mut request.reply_written=>if written.is_ok(){"written"}else{"unconfirmed"},
            completed=poll_session(&mut session),if session.is_some()=>{
                session_closed=completed.map_err(io::Error::other);
                session=None;
                "session_ended"
            },
            _=terminate.recv()=>{
                if let Ok(peers)=&peers{for peer in peers{peer.close();}}
                "cancelled"
            },
            _=interrupt.recv()=>{
                if let Ok(peers)=&peers{for peer in peers{peer.close();}}
                "cancelled"
            },
        };
        let previous = diagnostic.take();
        diagnostic = Some(
            async move {
                if let Some(previous) = previous {
                    previous.await;
                }
                as_types::diagnostics::record_stdout("AS_MAIN_STOP_REQUESTED".into()).await;
                as_types::diagnostics::record_stderr(format!(
                    "AS_MAIN_STOP_REPLY status={outcome}"
                ))
                .await;
            }
            .boxed(),
        );
    }

    // Keep polling the original subscriber while the source and its SQLite consumer retire.
    let retired = loop {
        tokio::select! {biased;
            retired=&mut retirement=>break retired,
            _=poll_diagnostic(&mut diagnostic),if diagnostic.is_some()=>{diagnostic=None;},
            completed=poll_session(&mut session),if session.is_some()=>{
                session_closed=completed.map_err(io::Error::other);
                session=None;
            },
            _=terminate.recv()=>{if let Ok(peers)=&peers{for peer in peers{peer.close();}}},
            _=interrupt.recv()=>{if let Ok(peers)=&peers{for peer in peers{peer.close();}}},
        }
    };
    let tail = business.chat_tail().await;
    if let Ok(peers) = &peers {
        for peer in peers {
            match &tail {
                Ok(tail) => peer.finish_chat_drain(*tail),
                Err(_) => peer.close(),
            }
        }
    }
    let local = async {
        match session.take() {
            Some(work) => work.await.map_err(io::Error::other),
            None => Ok(()),
        }
    };
    let draining = async {
        let (local, websocket) = tokio::join!(local, drain_websockets(&mut clients));
        local.and(websocket)
    };
    tokio::pin!(draining);
    let drained = loop {
        tokio::select! {biased;
            result=&mut draining=>break result,
            _=terminate.recv()=>{if let Ok(peers)=&peers{for peer in peers{peer.close();}}},
            _=interrupt.recv()=>{if let Ok(peers)=&peers{for peer in peers{peer.close();}}},
        }
    };
    session_closed = session_closed.and(drained);
    let peer_closed = media.close_peer();
    let management_closed = messages.close().map_err(io::Error::other);
    drop(endpoint);
    if let Some(diagnostic) = diagnostic.take() {
        diagnostic.await;
    }
    for (code, count) in session_errors {
        as_types::diagnostics::record_stderr(format!("{code} count={count}")).await;
    }
    let counts = business.counts().await;
    let closed = business.close().await;
    match assembly.log_storage().usage() {
        Ok(usage) => {
            as_types::diagnostics::record_stdout(format!(
                "AS_LOG_STORAGE_SUMMARY {}",
                serde_json::to_string(&usage)?
            ))
            .await
        }
        Err(error) => {
            as_types::diagnostics::record_stderr(format!(
                "AS_LOG_STORAGE_SUMMARY_FAILED code={:?}",
                error.kind()
            ))
            .await
        }
    }
    if let Ok(ref counts) = counts {
        as_types::diagnostics::record_stdout(format!(
            "AS_SCRIPTED_BUSINESS_SUMMARY {}",
            serde_json::to_string(counts)?
        ))
        .await;
    }
    as_types::diagnostics::record_stdout(format!(
        "AS_SCRIPTED_BUSINESS_WRITES media_committed={} failed={}",
        media.stored_records(),
        business.write_failure_count()
    ))
    .await;
    result?;
    peers?;
    management_closed?;
    peer_closed?;
    session_closed?;
    retired?;
    tail?;
    counts?;
    closed?;
    if business.write_failure_count() != 0 {
        return Err(io::Error::other("business database write failure retained").into());
    }
    as_types::diagnostics::record_stdout(format!(
        "AS_SCRIPTED_MEDIA_SUMMARY not_forwarded={}",
        media.not_forwarded()
    ))
    .await;
    as_types::diagnostics::record_stdout("AS_SCRIPTED_SERVICE_STOPPED".into()).await;
    Ok(())
}
