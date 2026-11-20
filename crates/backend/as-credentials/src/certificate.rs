use super::*;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use time::{Duration, OffsetDateTime};
fn err() -> CredentialError {
    CredentialError::InternalFailure
}
pub(super) fn public(ca: &Ca) -> CaPublicInfo {
    CaPublicInfo {
        id: ca.id,
        certificate_der: (*ca.der).clone(),
        fingerprint_sha256: Sha256::digest(ca.der.as_slice()).into(),
    }
}
pub(super) fn generate_ca() -> Result<Ca, CredentialError> {
    let key = KeyPair::generate().map_err(|_| err())?;
    let mut p = CertificateParams::new(Vec::<String>::new()).map_err(|_| err())?;
    p.distinguished_name
        .push(DnType::CommonName, "AgentShield Controlled Egress CA");
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
    p.not_after = OffsetDateTime::now_utc() + Duration::days(3650);
    let cert = p.self_signed(&key).map_err(|_| err())?;
    Ok(Ca {
        id: CaRef(random()?),
        der: Arc::new(cert.der().to_vec()),
        key: Arc::new(PrivateBytes::new(key.serialize_der())),
    })
}
fn issuer(ca: &Ca) -> Result<(rcgen::Certificate, KeyPair), CredentialError> {
    let key = KeyPair::try_from(ca.key.expose()).map_err(|_| err())?;
    let p = CertificateParams::from_ca_cert_der(&CertificateDer::from(ca.der.as_slice()))
        .map_err(|_| err())?;
    let issuer = p.self_signed(&key).map_err(|_| err())?;
    Ok((issuer, key))
}
pub(super) fn validate_ca(ca: &Ca) -> Result<(), CredentialError> {
    let _ = issuer(ca)?;
    let provider = rustls::crypto::ring::default_provider();
    let key = provider
        .key_provider
        .load_private_key(PrivatePkcs8KeyDer::from(ca.key.expose().to_vec()).into())
        .map_err(|_| err())?;
    rustls::sign::CertifiedKey::new(vec![CertificateDer::from((*ca.der).clone())], key)
        .keys_match()
        .map_err(|_| err())
}
pub(super) fn leaf(
    ca: &Ca,
    host: &str,
) -> Result<Arc<rustls::sign::CertifiedKey>, CredentialError> {
    let (issuer, key) = issuer(ca)?;
    let leaf_key = KeyPair::generate().map_err(|_| err())?;
    let mut p = CertificateParams::new(vec![host.to_owned()]).map_err(|_| err())?;
    p.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
    p.not_after = OffsetDateTime::now_utc() + Duration::days(1);
    let cert = p.signed_by(&leaf_key, &issuer, &key).map_err(|_| err())?;
    let signing = rustls::crypto::ring::default_provider()
        .key_provider
        .load_private_key(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into())
        .map_err(|_| err())?;
    let certified = rustls::sign::CertifiedKey::new(
        vec![cert.der().clone(), CertificateDer::from((*ca.der).clone())],
        signing,
    );
    certified.keys_match().map_err(|_| err())?;
    Ok(Arc::new(certified))
}
