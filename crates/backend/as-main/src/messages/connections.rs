//! Main-owned authenticated local sessions; channel retirement is separate from rule matching.
use as_policy_engine::interaction::{Connection as InteractionConnection, Interaction, Prompt};
use as_types::{
    configuration::{Access, MiddlewareAccess, Principal},
    main_ports::ModuleError,
};
use std::sync::{Arc, Mutex, Weak};
use tokio::net::UnixStream;
struct Grant {
    revocation: tokio_util::sync::CancellationToken,
    policy: MiddlewareAccess,
    revoked: bool,
    ready: Option<InteractionConnection>,
}
impl Grant {
    fn authorize(&self, required: Access) -> Result<(), ModuleError> {
        if self.revoked {
            return Err(ModuleError::Unauthenticated);
        }
        if self.policy.access < required {
            return Err(ModuleError::Forbidden);
        }
        Ok(())
    }

    fn review_ready(&self) -> Result<(), ModuleError> {
        self.authorize(Access::ManageRules)?;
        self.ready.as_ref().ok_or(ModuleError::Unavailable)?;
        Ok(())
    }
}
#[derive(Clone)]
pub(super) struct Authorization(Arc<Mutex<Grant>>);
type QuestionSlot = Mutex<Option<(Authorization, Prompt)>>;
pub(super) type PendingQuestion = Arc<QuestionSlot>;
impl Authorization {
    pub(super) async fn revoked(&self) {
        let token = match self.0.lock() {
            Ok(grant) => grant.revocation.clone(),
            Err(_) => return,
        };
        token.cancelled().await;
    }
    pub(super) fn chat(&self) -> Result<(), ModuleError> {
        let grant = self.0.lock().map_err(|_| ModuleError::Unavailable)?;
        if grant.revoked {
            return Err(ModuleError::Unauthenticated);
        }
        if !grant.policy.chat_content {
            return Err(ModuleError::Forbidden);
        }
        Ok(())
    }
    pub(super) fn check(&self, required: Access) -> Result<(), ModuleError> {
        self.0
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .authorize(required)
    }
    pub(super) fn same_connection(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
/// One actual authenticated transport. Its identity never comes from a message field.
pub struct Connection {
    local: Option<(UnixStream, u32)>,
    grant: Arc<Mutex<Grant>>,
    interaction: Arc<Interaction>,
    question: Weak<QuestionSlot>,
}
/// Main's connection lifetime bookkeeping, not a second configuration policy store.
pub(crate) struct Connections {
    interaction: Arc<Interaction>,
    sessions: Mutex<Vec<Weak<Mutex<Grant>>>>,
    question: Weak<QuestionSlot>,
}
impl Connections {
    /// Wire the rule owner's real availability interface.
    pub(super) fn new(interaction: Arc<Interaction>, question: &PendingQuestion) -> Self {
        Self {
            interaction,
            sessions: Mutex::new(Vec::new()),
            question: Arc::downgrade(question),
        }
    }
    #[cfg(test)]
    pub(crate) fn for_testing(interaction: Arc<Interaction>) -> Self {
        Self {
            interaction,
            sessions: Mutex::new(Vec::new()),
            question: Weak::new(),
        }
    }
    /// Authenticate once against configuration's current server policy.
    pub(crate) fn connect(
        &self,
        stream: UnixStream,
        policy: &MiddlewareAccess,
    ) -> Result<Connection, ModuleError> {
        let uid = stream
            .peer_cred()
            .map_err(|_| ModuleError::Unavailable)?
            .uid();
        if !matches!(policy.peer,Principal::UnixUid{uid:expected} if uid==expected) {
            return Err(ModuleError::Unauthenticated);
        }
        self.register(policy, Some((stream, uid)))
    }

    /// Register a WebSocket whose credential was verified under Main's admission lock.
    pub(super) fn connect_websocket(
        &self,
        policy: &MiddlewareAccess,
    ) -> Result<Connection, ModuleError> {
        self.register(policy, None)
    }

    fn register(
        &self,
        policy: &MiddlewareAccess,
        local: Option<(UnixStream, u32)>,
    ) -> Result<Connection, ModuleError> {
        let grant = Arc::new(Mutex::new(Grant {
            revocation: tokio_util::sync::CancellationToken::new(),
            policy: policy.clone(),
            revoked: false,
            ready: None,
        }));
        let mut sessions = self.sessions.lock().map_err(|_| ModuleError::Unavailable)?;
        sessions.retain(|v| v.strong_count() != 0);
        sessions.push(Arc::downgrade(&grant));
        Ok(Connection {
            local,
            grant,
            interaction: self.interaction.clone(),
            question: self.question.clone(),
        })
    }
    /// Actual authorization policy adoption retires previous grants and Ask availability immediately.
    pub(super) fn revoke_all(&self) -> Result<(), ModuleError> {
        let mut sessions = self.sessions.lock().map_err(|_| ModuleError::Unavailable)?;
        for session in sessions.iter().filter_map(Weak::upgrade) {
            let mut grant = session.lock().map_err(|_| ModuleError::Unavailable)?;
            grant.revoked = true;
            grant.revocation.cancel();
            grant.ready.take();
        }
        sessions.clear();
        self.retire_question()
    }

    pub(super) fn stop_review(&self) -> Result<(), ModuleError> {
        let sessions = self.sessions.lock().map_err(|_| ModuleError::Unavailable)?;
        for session in sessions.iter().filter_map(Weak::upgrade) {
            session
                .lock()
                .map_err(|_| ModuleError::Unavailable)?
                .ready
                .take();
        }
        self.retire_question()
    }

    fn retire_question(&self) -> Result<(), ModuleError> {
        if let Some(question) = self.question.upgrade() {
            question
                .lock()
                .map_err(|_| ModuleError::Unavailable)?
                .take();
        }
        Ok(())
    }
}
impl Connection {
    /// Check per-command authorization without re-reading kernel peer credentials.
    pub fn authorize(&self, required: Access) -> Result<(), ModuleError> {
        self.authorization().check(required)
    }
    pub(super) fn authorization(&self) -> Authorization {
        Authorization(self.grant.clone())
    }
    /// Require the actual registered decision channel on this connection.
    pub fn review_ready(&self) -> Result<(), ModuleError> {
        self.grant
            .lock()
            .map_err(|_| ModuleError::Unavailable)?
            .review_ready()
    }
    /// Observe the live review channel for routing without retaining the connection or grant.
    pub fn review_readiness(&self) -> impl Fn() -> bool + Send + Sync + 'static + use<> {
        let grant = Arc::downgrade(&self.grant);
        move || {
            grant
                .upgrade()
                .is_some_and(|grant| grant.lock().is_ok_and(|grant| grant.review_ready().is_ok()))
        }
    }
    /// Check the current original connection's permission to receive chat content.
    pub fn authorize_content(&self) -> Result<(), ModuleError> {
        self.authorization().chat()
    }
    /// Wait for actual connection or policy revocation; does not extend its lifetime.
    pub async fn revoked(&self) {
        self.authorization().revoked().await;
    }
    /// Register only a real decision channel; revoked grants cannot become ready again.
    pub(crate) fn set_review_available(&self, available: bool) -> Result<(), ModuleError> {
        let mut grant = self.grant.lock().map_err(|_| ModuleError::Unavailable)?;
        grant.authorize(Access::ManageRules)?;
        if available && grant.ready.is_none() {
            grant.ready = Some(self.interaction.connect()?);
        } else if !available {
            grant.ready.take();
        }
        drop(grant);
        if !available {
            self.retire_question();
        }
        Ok(())
    }
    fn retire_question(&self) {
        if let Some(question) = self.question.upgrade() {
            let mut pending = match question.lock() {
                Ok(pending) => pending,
                Err(poison) => poison.into_inner(),
            };
            if pending
                .as_ref()
                .is_some_and(|(grant, _)| Arc::ptr_eq(&grant.0, &self.grant))
            {
                pending.take();
            }
        }
    }
    /// Same verified underlying connection.
    pub fn stream(&self) -> Option<&UnixStream> {
        self.local.as_ref().map(|(stream, _)| stream)
    }
    /// OS-confirmed UID for observation only.
    pub fn uid(&self) -> Option<u32> {
        self.local.as_ref().map(|(_, uid)| *uid)
    }

    /// Effective permissions of this live connection.
    pub fn permissions(&self) -> Result<(Access, bool), ModuleError> {
        let grant = self.grant.lock().map_err(|_| ModuleError::Unavailable)?;
        if grant.revoked {
            return Err(ModuleError::Unauthenticated);
        }
        Ok((grant.policy.access, grant.policy.chat_content))
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        // Queued command tokens must not prolong the actual transport's authorization/availability.
        if let Ok(mut grant) = self.grant.lock() {
            grant.revoked = true;
            grant.revocation.cancel();
            grant.ready.take();
        }
        self.retire_question();
    }
}
