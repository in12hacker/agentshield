use super::*;
use rustix::{
    fd::OwnedFd,
    fs::{self, AtFlags, FileType, Mode, OFlags, ResolveFlags},
};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    path::{Component, Path},
};
use zeroize::Zeroizing;
const LIMIT: u64 = 32 * 1024 * 1024;
fn err() -> CredentialError {
    CredentialError::InternalFailure
}
pub(super) struct Storage {
    dir: OwnedFd,
    _lock: OwnedFd,
    store_name: OsString,
    public_name: OsString,
    #[cfg(test)]
    path: PathBuf,
}
fn valid_stat(m: &fs::Stat, public: bool) -> Result<(), CredentialError> {
    if FileType::from_raw_mode(m.st_mode) != FileType::RegularFile
        || m.st_uid != rustix::process::geteuid().as_raw()
        || m.st_nlink != 1
        || m.st_size < 0
        || m.st_size as u64 > LIMIT
        || m.st_mode & if public { 0o022 } else { 0o077 } != 0
    {
        return Err(err());
    }
    Ok(())
}
impl Storage {
    pub(super) fn open(root: &Path, store: &Path, public: &Path) -> Result<Self, CredentialError> {
        if store.parent() != public.parent() {
            return Err(err());
        }
        let parent = store
            .parent()
            .ok_or_else(err)?
            .strip_prefix(root)
            .map_err(|_| err())?;
        let mut dir = fs::openat2(
            fs::CWD,
            root,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::NO_SYMLINKS,
        )
        .map_err(|_| err())?;
        let root_stat = fs::fstat(&dir).map_err(|_| err())?;
        if root_stat.st_uid != rustix::process::geteuid().as_raw() || root_stat.st_mode & 0o022 != 0
        {
            return Err(err());
        }
        let components = parent.components().collect::<Vec<_>>();
        if components.is_empty() {
            return Err(err());
        }
        for (index, c) in components.iter().enumerate() {
            let Component::Normal(name) = c else {
                return Err(err());
            };
            match fs::mkdirat(&dir, *name, Mode::from_raw_mode(0o700)) {
                Ok(()) => fs::fsync(&dir).map_err(|_| err())?,
                Err(rustix::io::Errno::EXIST) => {}
                Err(_) => return Err(err()),
            }
            dir = fs::openat(
                &dir,
                *name,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|_| err())?;
            let stat = fs::fstat(&dir).map_err(|_| err())?;
            if stat.st_uid != rustix::process::geteuid().as_raw()
                || stat.st_mode
                    & if index + 1 == components.len() {
                        0o077
                    } else {
                        0o022
                    }
                    != 0
            {
                return Err(err());
            }
        }
        let lock = fs::openat(
            &dir,
            ".lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )
        .map_err(|_| err())?;
        valid_stat(&fs::fstat(&lock).map_err(|_| err())?, false)?;
        fs::flock(&lock, fs::FlockOperation::NonBlockingLockExclusive).map_err(|_| err())?;
        Ok(Self {
            dir,
            _lock: lock,
            store_name: store.file_name().ok_or_else(err)?.to_owned(),
            public_name: public.file_name().ok_or_else(err)?.to_owned(),
            #[cfg(test)]
            path: store.to_owned(),
        })
    }
    fn stat(&self, name: &OsString) -> Result<Option<fs::Stat>, CredentialError> {
        match fs::statat(&self.dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(v) => Ok(Some(v)),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(_) => Err(err()),
        }
    }
    fn read(&self, name: &OsString, public: bool) -> Result<Zeroizing<Vec<u8>>, CredentialError> {
        let fd = fs::openat(
            &self.dir,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| err())?;
        valid_stat(&fs::fstat(&fd).map_err(|_| err())?, public)?;
        let mut bytes = Zeroizing::new(Vec::new());
        File::from(fd)
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| err())?;
        if bytes.len() as u64 > LIMIT {
            return Err(err());
        }
        Ok(bytes)
    }
    fn atomic_write(
        &self,
        name: &OsString,
        bytes: &[u8],
        public: bool,
    ) -> Result<bool, CredentialError> {
        if let Some(m) = self.stat(name)? {
            valid_stat(&m, public)?
        }
        let tmp = OsString::from(format!(
            ".as-credential-{}",
            URL_SAFE_NO_PAD.encode(random::<16>()?)
        ));
        let result = (|| {
            let fd = fs::openat(
                &self.dir,
                &tmp,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(if public { 0o644 } else { 0o600 }),
            )
            .map_err(|_| err())?;
            let mut file = File::from(fd);
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| err())?;
            fs::renameat(&self.dir, &tmp, &self.dir, name).map_err(|_| err())?;
            let durable = fs::fsync(&self.dir).is_ok();
            #[cfg(test)]
            let durable = durable && !crate::tests::force_sync_failure(&self.path);
            Ok(durable)
        })();
        if result.is_err() {
            let _ = fs::unlinkat(&self.dir, &tmp, AtFlags::empty());
        }
        result
    }
}
pub(super) fn load(storage: &Storage) -> Result<Store, CredentialError> {
    if storage.stat(&storage.store_name)?.is_none() {
        if storage.stat(&storage.public_name)?.is_some() {
            return Err(CredentialError::SecretUnavailable);
        }
        let s = Store {
            instance: random()?,
            revision: 0,
            secrets: HashMap::new(),
            tokens: HashMap::new(),
            ca: None,
        };
        if !save(storage, &s)? {
            return Err(CredentialError::PersistenceUncertain);
        }
        return Ok(s);
    }
    let bytes = storage.read(&storage.store_name, false)?;
    let mut j: Value = serde_json::from_slice(&bytes).map_err(|_| err())?;
    let result = decode(&j);
    wipe_json(&mut j);
    let store = result?;
    if let Some(ca) = &store.ca {
        certificate::validate_ca(ca)?
    } else if storage.stat(&storage.public_name)?.is_some() {
        return Err(CredentialError::SecretUnavailable);
    }
    Ok(store)
}
fn wipe_json(value: &mut Value) {
    use zeroize::Zeroize;
    match value {
        Value::String(v) => v.zeroize(),
        Value::Array(v) => v.iter_mut().for_each(wipe_json),
        Value::Object(v) => v.values_mut().for_each(wipe_json),
        _ => {}
    }
}
fn id<const N: usize>(j: &Value) -> Result<[u8; N], CredentialError> {
    let b = URL_SAFE_NO_PAD
        .decode(j.as_str().ok_or_else(err)?)
        .map_err(|_| err())?;
    b.try_into().map_err(|_| err())
}
fn decode(j: &Value) -> Result<Store, CredentialError> {
    let mut s = Store {
        instance: id(&j["instance"])?,
        revision: j["revision"].as_u64().ok_or_else(err)?,
        secrets: HashMap::new(),
        tokens: HashMap::new(),
        ca: None,
    };
    let secrets = j["secrets"].as_array().ok_or_else(err)?;
    let tokens = j["tokens"].as_array().ok_or_else(err)?;
    if secrets.len() > 1024 || tokens.len() > 8192 {
        return Err(err());
    }
    for r in secrets {
        let secret = SecretRef(id(&r["id"])?);
        let value = PrivateBytes::new(
            URL_SAFE_NO_PAD
                .decode(r["value"].as_str().ok_or_else(err)?)
                .map_err(|_| err())?,
        );
        validate_secret(value.expose())?;
        let revision = r["revision"].as_u64().filter(|n| *n > 0).ok_or_else(err)?;
        if s.secrets
            .insert(
                secret,
                Secret {
                    revision,
                    value: Arc::new(value),
                },
            )
            .is_some()
        {
            return Err(err());
        }
    }
    for r in tokens {
        let token = PhantomTokenId(id(&r["id"])?);
        let digest = id(&r["digest"])?;
        if s.tokens.values().any(|v| v.digest == digest) {
            return Err(err());
        }
        if s.tokens
            .insert(
                token,
                Token {
                    binding: BindingId(id(&r["binding"])?),
                    digest,
                    enabled: r["enabled"].as_bool().ok_or_else(err)?,
                },
            )
            .is_some()
        {
            return Err(err());
        }
    }
    if !j["ca"].is_null() {
        let r = &j["ca"];
        s.ca = Some(Ca {
            id: CaRef(id(&r["id"])?),
            der: Arc::new(
                URL_SAFE_NO_PAD
                    .decode(r["der"].as_str().ok_or_else(err)?)
                    .map_err(|_| err())?,
            ),
            key: Arc::new(PrivateBytes::new(
                URL_SAFE_NO_PAD
                    .decode(r["key"].as_str().ok_or_else(err)?)
                    .map_err(|_| err())?,
            )),
        })
    }
    Ok(s)
}
pub(super) fn save(storage: &Storage, s: &Store) -> Result<bool, CredentialError> {
    #[cfg(test)]
    crate::tests::before_save(&storage.path);
    // Explicit private file encoding: secret types have no Serialize implementation.
    let mut j = json!({"instance":URL_SAFE_NO_PAD.encode(s.instance),"revision":s.revision,"secrets":s.secrets.iter().map(|(id,v)|json!({"id":URL_SAFE_NO_PAD.encode(id.0),"revision":v.revision,"value":URL_SAFE_NO_PAD.encode(v.value.expose())})).collect::<Vec<_>>(),"tokens":s.tokens.iter().map(|(id,v)|json!({"id":URL_SAFE_NO_PAD.encode(id.0),"binding":URL_SAFE_NO_PAD.encode(v.binding.0),"digest":URL_SAFE_NO_PAD.encode(v.digest),"enabled":v.enabled})).collect::<Vec<_>>(),"ca":s.ca.as_ref().map(|v|json!({"id":URL_SAFE_NO_PAD.encode(v.id.0),"der":URL_SAFE_NO_PAD.encode(v.der.as_slice()),"key":URL_SAFE_NO_PAD.encode(v.key.expose())}))});
    let bytes = serde_json::to_vec(&j)
        .map(Zeroizing::new)
        .map_err(|_| err());
    wipe_json(&mut j);
    let bytes = bytes?;
    if bytes.len() as u64 > LIMIT {
        return Err(err());
    }
    storage.atomic_write(&storage.store_name, &bytes, false)
}
pub(super) fn save_public(storage: &Storage, der: &[u8]) -> Result<(), CredentialError> {
    use base64::engine::general_purpose::STANDARD;
    let b = STANDARD.encode(der);
    let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in b.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).map_err(|_| err())?);
        pem.push('\n')
    }
    pem.push_str("-----END CERTIFICATE-----\n");
    if storage.stat(&storage.public_name)?.is_some() {
        if storage.read(&storage.public_name, true)?.as_slice() != pem.as_bytes() {
            return Err(CredentialError::SecretUnavailable);
        }
        return Ok(());
    }
    if storage.atomic_write(&storage.public_name, pem.as_bytes(), true)? {
        Ok(())
    } else {
        Err(CredentialError::PersistenceUncertain)
    }
}
