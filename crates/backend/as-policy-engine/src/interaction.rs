//! Bounded interactive requests. No lock is held while waiting for a user or original deadline.
use as_types::{DecisionId, dispatch_event::EventData, main_ports::*};
use std::{sync::Arc, time::Duration};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Copy, Default)]
struct Channels {
    count: usize,
    disconnect_epoch: u64,
}

/// One UI/IM request with a response capability for this original operation only.
pub struct Prompt {
    /// Original decision identity.
    pub decision_id: DecisionId,
    /// Facts used by the matching rule.
    pub event: Arc<EventData>,
    /// Matched rule, including exact version and logging selection.
    pub hit: RuleHit,
    /// Effective monotonic deadline; relay sends remaining time, never serializes Instant.
    pub deadline: tokio::time::Instant,
    /// Timeout decision accepted by the original operation.
    pub timeout: Decision,
    /// User reply for this request; sending after retirement fails.
    pub reply: oneshot::Sender<Decision>,
}
impl Prompt {
    /// The current ordinary review actions, in presentation order.
    /// The rule's separately bound fallback is never an offered action.
    pub fn choices(&self) -> &'static [Decision] {
        &[
            Decision::Allow,
            Decision::Deny {
                terminate: Termination::None,
            },
        ]
    }
}
/// Lifetime of one real interactive channel. Dropping the last one retires existing waits.
pub struct Connection {
    state: watch::Sender<Channels>,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.state.send_modify(|s| {
            s.count -= 1;
            if s.count == 0 {
                s.disconnect_epoch = s.disconnect_epoch.wrapping_add(1);
            }
        });
    }
}
/// Service-side port. Attach connections only when an actual interaction channel is ready.
pub struct Interaction {
    requests: mpsc::Sender<Prompt>,
    state: watch::Sender<Channels>,
}
impl Interaction {
    /// Create the bounded outgoing request port; no connection is assumed present.
    pub fn channel(capacity: std::num::NonZeroUsize) -> (Arc<Self>, mpsc::Receiver<Prompt>) {
        let (requests, receiving) = mpsc::channel(capacity.get());
        let (state, _) = watch::channel(Channels::default());
        (Arc::new(Self { requests, state }), receiving)
    }
    /// True only while at least one authenticated decision channel is ready.
    pub fn available(&self) -> bool {
        self.state.borrow().count != 0
    }
    /// Message-module callback after channel authentication; this only registers availability.
    pub fn connect(&self) -> Result<Connection, ModuleError> {
        let mut added = false;
        self.state.send_modify(|s| {
            if let Some(count) = s.count.checked_add(1) {
                s.count = count;
                added = true;
            }
        });
        if !added {
            return Err(ModuleError::Unavailable);
        }
        Ok(Connection {
            state: self.state.clone(),
        })
    }
    pub(crate) async fn ask(
        &self,
        request: ReviewRequest<'_>,
        hit: RuleHit,
        timeout_ms: u64,
        timeout: Decision,
    ) -> Result<(Decision, ChoiceReason), ModuleError> {
        let mut stop = request.operation.stop_signal();
        let mut channels = self.state.subscribe();
        let start = *channels.borrow_and_update();
        if start.count == 0 {
            return Ok((timeout, ChoiceReason::Timeout));
        }
        let configured = tokio::time::Instant::now().checked_add(Duration::from_millis(timeout_ms));
        let deadline = configured.map_or(request.operation.hold_window().deadline(), |v| {
            v.min(request.operation.hold_window().deadline())
        });
        if deadline <= tokio::time::Instant::now() {
            return Ok((timeout, ChoiceReason::Timeout));
        }
        let waiting = async {
            request
                .operation
                .bind_review(as_types::operation::ReviewBinding {
                    hit: hit.clone(),
                    deadline,
                    timeout,
                })
                .await?;
            let (reply, answered) = oneshot::channel();
            {
                // Publication is linearized with channel retirement; never hold this guard across await.
                let current = self.state.borrow();
                if current.count == 0
                    || current.disconnect_epoch != start.disconnect_epoch
                    || tokio::time::Instant::now() >= deadline
                {
                    return Ok((timeout, ChoiceReason::Timeout));
                }
                if self
                    .requests
                    .try_send(Prompt {
                        decision_id: request.operation.decision_id(),
                        event: request.event.clone(),
                        hit,
                        deadline,
                        timeout,
                        reply,
                    })
                    .is_err()
                {
                    return Ok((timeout, ChoiceReason::Timeout));
                }
            }
            Ok(match answered.await {
                Ok(decision) => (decision, ChoiceReason::User),
                Err(_) => (timeout, ChoiceReason::Timeout),
            })
        };
        tokio::pin!(waiting);
        loop {
            let now = *channels.borrow_and_update();
            if stop.as_ref().is_some_and(|v| {
                *v.borrow() == Some(as_types::operation::StopReason::SourceFailure)
            }) {
                return Err(ModuleError::Unavailable);
            }
            if now.count == 0
                || now.disconnect_epoch != start.disconnect_epoch
                || stop.as_ref().is_some_and(|v| v.borrow().is_some())
            {
                return Ok((timeout, ChoiceReason::Timeout));
            }
            tokio::select! { biased;
                _ = tokio::time::sleep_until(deadline) => return Ok((timeout, ChoiceReason::Timeout)),
                changed = channels.changed() => if changed.is_err() { return Err(ModuleError::Unavailable); },
                changed = async { match stop.as_mut() { Some(v) => v.changed().await.map_err(|_| ModuleError::Unavailable), None => std::future::pending().await } } => { changed?; },
                result = &mut waiting => return result,
            }
        }
    }
}
