//! Actual loader process lifetime; unexpected EOF leaves native BPF pins in force.
use as_types::{
    driver::{BPF_OBJECT_FILE_NAME, LOADER_FILE_NAME},
    main_ports::ModuleError,
};
use rustix::fs::{Mode, OFlags, ResolveFlags, openat2};
use std::{
    fs::File,
    io::Read,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::Path,
    process::Stdio,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, Command},
};

pub(crate) struct Loader {
    child: Child,
    input: Option<ChildStdin>,
    // Keep the exact reviewed object available to the child's /proc/PARENT/fd argument.
    _object: File,
    _program: File,
}
pub(crate) fn asset(directory: &File, name: &str) -> Result<File, ModuleError> {
    let file = File::from(
        openat2(
            directory,
            name,
            OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
            ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
        )
        .map_err(|_| ModuleError::Unavailable)?,
    );
    let meta = file.metadata().map_err(|_| ModuleError::Unavailable)?;
    if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o022 != 0 {
        return Err(ModuleError::Unauthenticated);
    }
    Ok(file)
}
impl Loader {
    pub(crate) async fn start(path: &Path, instance: [u8; 16]) -> Result<Self, ModuleError> {
        let path = path.to_path_buf();
        let (program, object) = tokio::task::spawn_blocking(move || {
            let directory = File::from(
                openat2(
                    rustix::fs::CWD,
                    &path,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                    Mode::empty(),
                    ResolveFlags::NO_SYMLINKS,
                )
                .map_err(|_| ModuleError::Unavailable)?,
            );
            let meta = directory.metadata().map_err(|_| ModuleError::Unavailable)?;
            if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                return Err(ModuleError::Unauthenticated);
            }
            let mut program = asset(&directory, LOADER_FILE_NAME)?;
            let object = asset(&directory, BPF_OBJECT_FILE_NAME)?;
            let mut magic = [0; 4];
            program
                .read_exact(&mut magic)
                .map_err(|_| ModuleError::Unavailable)?;
            if magic != *b"\x7fELF" {
                return Err(ModuleError::InvalidInput);
            }
            Ok((program, object))
        })
        .await
        .map_err(|_| ModuleError::Unavailable)??;
        let mut hex = String::with_capacity(32);
        for byte in instance {
            use std::fmt::Write;
            write!(&mut hex, "{byte:02x}").map_err(|_| ModuleError::Unavailable)?;
        }
        let executable = format!("/proc/self/fd/{}", program.as_raw_fd());
        let object_path = format!("/proc/{}/fd/{}", std::process::id(), object.as_raw_fd());
        let mut child = Command::new(executable)
            .arg(object_path)
            .arg(hex)
            .current_dir("/")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(false)
            .spawn()
            .map_err(|error| {
                tracing::error!(stage="spawn_bpf_loader",errno=?error.raw_os_error(),%error,"loader process failed");
                ModuleError::Unavailable
            })?;
        let input = child.stdin.take().ok_or(ModuleError::Unavailable)?;
        let mut output = child.stdout.take().ok_or(ModuleError::Unavailable)?;
        let mut ready = [0; 13];
        if output.read_exact(&mut ready).await.is_err() || ready != *b"AS-BPF-READY\n" {
            drop(input);
            let exit = child.wait().await;
            tracing::error!(
                stage = "bpf_loader_readiness",
                ?exit,
                "loader did not confirm full hook attachment"
            );
            return Err(ModuleError::Unavailable);
        }
        Ok(Self {
            child,
            input: Some(input),
            _object: object,
            _program: program,
        })
    }
    pub(crate) async fn finish(mut self, retire: bool) -> Result<(), ModuleError> {
        let mut sent = Ok(());
        if let Some(mut input) = self.input.take() {
            if retire {
                sent = input
                    .write_all(b"U")
                    .await
                    .map_err(|_| ModuleError::Unavailable);
            }
            drop(input);
        }
        let status = self
            .child
            .wait()
            .await
            .map_err(|_| ModuleError::Unavailable)?;
        sent?;
        if !status.success() {
            return Err(ModuleError::Unavailable);
        }
        Ok(())
    }
}
// ChildStdin Drop supplies EOF; it deliberately does not turn a crashed owner into an unpin request.
