//! Single operation identifiers used by event producers, rule compilers and consumers.
use serde::{Deserialize, Serialize};

/// Process events; creation and exit are observations, not executable rule proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum ProcessOperation {
    /// Execute an image, or observe its confirmed successful execution.
    Execute = 1,
    /// A new process group was actually created.
    Create = 2,
    /// The last live thread entered the process group's exit path.
    Exit = 3,
}
/// File operations, including pipe access without claiming message-level control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum FileOperation {
    /// Open permission.
    Open = 1,
    /// An actual read permission check.
    Read = 2,
    /// An actual write permission check.
    Write = 3,
    /// Named regular file creation permission. Also used by the system self-protection result.
    CreateFile = 4,
    /// Directory creation permission.
    CreateDirectory = 5,
    /// Removal of a file name.
    Unlink = 6,
    /// Removal of a directory.
    RemoveDirectory = 7,
    /// Creation of a hard link, retaining both original paths.
    Link = 8,
    /// Rename, including both checked paths.
    Rename = 9,
    /// Truncation of the actual open file object.
    TruncateOpenFile = 10,
    /// Mode changes.
    ChangeMode = 11,
    /// Owner changes.
    ChangeOwner = 12,
    /// Write permission upgrade of an existing shared mapping, checked by self-protection.
    GrantWritableMapping = 13,
    /// Mounting on a path.
    Mount = 14,
    /// Unmounting a filesystem.
    Unmount = 15,
    /// Moving a mount between checked paths.
    MoveMount = 16,
    /// Other explicitly checked inode metadata modifications.
    SetMetadata = 17,
    /// Truncation at the original path-based permission check.
    TruncatePath = 18,
    /// Admission of a file-backed shared mapping with effective write permission.
    MapSharedWritable = 19,
}
impl FileOperation {
    pub(crate) const fn is_defined_code(code: u32) -> bool {
        code >= Self::Open as u32 && code <= Self::MapSharedWritable as u32
    }
}
/// Network operations at their original socket check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum NetworkOperation {
    /// Connect a socket.
    Connect = 1,
    /// Send on a socket.
    Send = 2,
}
