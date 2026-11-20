//! Exact u64 JSON strings for process DTOs consumed by JavaScript clients.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

#[derive(Deserialize, Serialize)]
#[serde(transparent)]
struct Exact(#[serde(with = "crate::decimal")] u64);

#[derive(Serialize)]
#[serde(transparent)]
struct ExactList<'a>(#[serde(serialize_with = "list::serialize")] &'a [u64]);

pub(crate) mod list {
    use super::Exact;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(value.iter().copied().map(Exact))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u64>, D::Error> {
        Ok(Vec::<Exact>::deserialize(d)?
            .into_iter()
            .map(|v| v.0)
            .collect())
    }
}

pub(crate) mod optional_list {
    use super::{Exact, ExactList};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(
        value: &Option<Vec<u64>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(values) => serializer.serialize_some(&ExactList(values)),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u64>>, D::Error> {
        Ok(Option::<Vec<Exact>>::deserialize(d)?
            .map(|values| values.into_iter().map(|v| v.0).collect()))
    }
}
/// Exact nullable unsigned counters and timestamps.
pub mod optional {
    use super::Exact;
    use serde::{Deserialize, Deserializer, Serializer};
    /// Preserve all bits in an optional value.
    pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
        match value {
            Some(value) => serializer.serialize_some(&value.to_string()),
            None => serializer.serialize_none(),
        }
    }
    /// Decode canonical optional decimal strings.
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        Ok(Option::<Exact>::deserialize(deserializer)?.map(|value| value.0))
    }
}
/// Serialize every bit as a canonical decimal string.
pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&value.to_string())
}
/// Accept only a canonical unsigned decimal string, without numeric precision loss.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty()
        || !value.bytes().all(|b| b.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(D::Error::custom("expected canonical u64 decimal string"));
    }
    value.parse().map_err(D::Error::custom)
}
