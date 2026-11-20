//! Main-owned runtime access; business history and source queries belong to middleware.
use super::{
    MainMessages,
    connections::{Authorization, Connection},
};
use as_types::{configuration::Access, flow::*, main_ports::ModuleError};
use std::sync::Arc;

/// Main composes only actual runtime/control owners, without a content store.
#[derive(Clone)]
pub struct FlowPorts {
    pub ingress: Option<Arc<dyn as_types::driver::TlsIngressSession>>,
    pub status: Option<Arc<dyn FlowStatusPort>>,
    pub credentials: as_credentials::CredentialModule,
}
/// Original authenticated content grant; no message cache or database is held here.
pub struct ContentAccess {
    authorization: Authorization,
}
impl ContentAccess {
    pub fn authorize(&self) -> Result<(), ModuleError> {
        self.authorization.chat()
    }
}
impl MainMessages {
    pub fn content_access(&self, session: &Connection) -> Result<ContentAccess, ModuleError> {
        session.authorization().chat()?;
        Ok(ContentAccess {
            authorization: session.authorization(),
        })
    }
    pub fn credential_admin(
        self: &Arc<Self>,
        session: &Connection,
    ) -> Result<AuthorizedCredentialAdmin, ModuleError> {
        session.authorize(Access::ManageService)?;
        Ok(AuthorizedCredentialAdmin {
            authorization: session.authorization(),
            owner: self.clone(),
        })
    }
}
/// Live grant wrapper prevents a retained management port from surviving connection revocation.
pub struct AuthorizedCredentialAdmin {
    authorization: Authorization,
    owner: Arc<MainMessages>,
}
impl AuthorizedCredentialAdmin {
    fn check(&self) -> Result<(), CredentialError> {
        self.authorization
            .check(Access::ManageService)
            .map_err(|_| CredentialError::ScopeDenied)
    }
    async fn execute<R, F, Fut>(&self, work: F) -> Result<R, CredentialError>
    where
        R: Send + 'static,
        F: FnOnce(Arc<dyn CredentialAdminPort>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<R, CredentialError>> + Send + 'static,
    {
        self.check()?;
        self.owner
            .credential_task(self.authorization.clone(), work)?
            .await
            .map_err(|_| CredentialError::Stopped)?
    }
    /// Create or export the same persistent CA.
    pub async fn ensure_ca(&self) -> Result<CaPublicInfo, CredentialError> {
        self.check()?;
        self.execute(|port| async move { port.ensure_ca().await })
            .await
    }
    /// Export only the public certificate through the original authorized credential owner.
    pub fn public_ca(&self, ca: CaRef) -> Result<CaPublicInfo, CredentialError> {
        self.check()?;
        self.owner
            .flow
            .as_ref()
            .ok_or(CredentialError::SecretUnavailable)?
            .credentials
            .certificate_port()
            .public_ca(ca)
    }
    /// Persist actual provider authentication material through its only writer.
    pub async fn import_secret(
        &self,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        self.check()?;
        if value.expose().len() > as_types::flow::MAX_SECRET_BYTES {
            return Err(CredentialError::InvalidHeader);
        }
        self.execute(|port| async move { port.import_secret(value).await })
            .await
    }
    /// Rotate one existing secret revision.
    pub async fn replace_secret(
        &self,
        secret: SecretRef,
        expected: u64,
        value: PrivateBytes,
    ) -> Result<SecretWriteReceipt, CredentialError> {
        self.check()?;
        if value.expose().len() > as_types::flow::MAX_SECRET_BYTES {
            return Err(CredentialError::InvalidHeader);
        }
        self.execute(move |port| async move { port.replace_secret(secret, expected, value).await })
            .await
    }
    /// Return a newly issued phantom once.
    pub async fn issue_phantom(
        &self,
        binding: BindingId,
        expected: as_types::configuration::ConfigVersion,
    ) -> Result<PhantomIssue, CredentialError> {
        self.check()?;
        let issue = self
            .execute(move |port| async move { port.issue_phantom(binding, expected).await })
            .await?;
        self.check()?;
        Ok(issue)
    }
    /// Persist revocation before acknowledging it.
    pub async fn revoke_phantom(&self, token: PhantomTokenId) -> Result<u64, CredentialError> {
        self.check()?;
        self.execute(move |port| async move { port.revoke_phantom(token).await })
            .await
    }
}
