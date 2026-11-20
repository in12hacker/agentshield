//! Original security-audit facts shared by the driver, dispatcher and log consumer.
use serde::{Deserialize, Serialize};

/// Release of a regular file object opened for writing, without rule or actor attribution.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileCloseAuditRecord {
    /// The last reference to this writable file object was released.
    FileWriteClose {
        /// Actual host thread executing release; not the opener, writer or causal actor.
        reporter_tid: i32,
        /// Original path bytes resolved from the held path when AS reads the notification.
        path: Option<Vec<u8>>,
        /// Actual negative Linux errno from that path read; None after success.
        #[serde(deserialize_with = "Option::<i32>::deserialize")]
        path_error: Option<i32>,
        /// Device major value sampled from the original inode at release.
        device_major: u32,
        /// Device minor value sampled from the original inode at release.
        device_minor: u32,
        /// Original inode number sampled at release, encoded as decimal text.
        #[serde(with = "crate::decimal")]
        inode: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::FileCloseAuditRecord;

    #[test]
    fn current_record_preserves_empty_path_and_requires_path_error() {
        let record = FileCloseAuditRecord::FileWriteClose {
            reporter_tid: 0,
            path: Some(Vec::new()),
            path_error: None,
            device_major: 0,
            device_minor: 0,
            inode: u64::MAX,
        };
        let mut value = serde_json::to_value(record).expect("original fact");
        assert_eq!(value["path"], serde_json::json!([]));
        assert_eq!(value["path_error"], serde_json::Value::Null);
        assert_eq!(value["inode"], u64::MAX.to_string());
        let decoded: FileCloseAuditRecord =
            serde_json::from_value(value.clone()).expect("current fact");
        assert_eq!(serde_json::to_value(decoded).expect("read back"), value);
        value
            .as_object_mut()
            .expect("fact fields")
            .remove("path_error");
        assert!(serde_json::from_value::<FileCloseAuditRecord>(value).is_err());
    }
}
