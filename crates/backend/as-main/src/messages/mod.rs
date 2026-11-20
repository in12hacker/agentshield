//! Main-owned upper and lower ports. Module functions are composed here rather than reimplemented.
mod access;
pub mod connections;
pub mod driver;
mod management;
pub mod protocol;
pub use access::{AuthorizedCredentialAdmin, ContentAccess, FlowPorts};
pub use management::MainMessages;

/// One original message and its nonserializable, noncloneable local reply-write capability.
pub struct CommandReply {
    /// The sole current wire DTO; only this value is serialized.
    pub message: protocol::Up,
    /// Present only for a host-accepted RequestStop; move into the original output work.
    pub stop_reply_write: Option<crate::runtime::StopReplyWrite>,
}
