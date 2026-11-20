//! JSON type fidelity through the locked Serde decoder and borrowed original lexemes.
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Number, Value, value::RawValue};
use std::fmt;

/// Decode original JSON with Serde's default nesting guard and reject duplicate decoded keys.
///
/// Validation produces no `Value`; interpretation borrows raw child slices, scanning them again
/// at each nesting level. Work therefore scales with input length times nesting depth. Only the
/// resulting value is retained, including Serde's arbitrary-precision numbers and literal maps.
/// Number spelling follows the locked decoder (`-0` becomes `0`, `1e400` becomes `1e+400`);
/// original input bytes remain caller-owned.
pub fn parse(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    Syntax::deserialize(&mut decoder)?;
    decoder.end()?;
    value(bytes.trim_ascii_start())
}

// RawValue skips nested values internally. First visit their real containers with one decoder
// so its existing recursion guard remains effective; no independent depth threshold is added.
struct Syntax;
impl<'de> Deserialize<'de> for Syntax {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        decoder.deserialize_any(Self)
    }
}
macro_rules! scalar {
    ($($method:ident: $ty:ty),* $(,)?) => {$(
        fn $method<E: de::Error>(self, _: $ty) -> Result<Self::Value, E> {
            Ok(Self)
        }
    )*};
}
impl<'de> Visitor<'de> for Syntax {
    type Value = Self;
    fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("a JSON value")
    }
    scalar! {
        visit_bool: bool, visit_i64: i64, visit_u64: u64, visit_f64: f64,
        visit_str: &str,
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Self)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        while sequence.next_element::<Self>()?.is_some() {}
        Ok(Self)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_key::<de::IgnoredAny>()?.is_some() {
            map.next_value::<Self>()?;
        }
        Ok(Self)
    }
}

fn value(raw: &[u8]) -> Result<Value, serde_json::Error> {
    let mut decoder = serde_json::Deserializer::from_slice(raw);
    // A literal map must use deserialize_map, never Value's private-number map dispatch.
    let output = match raw.first() {
        Some(b'{') => decoder.deserialize_map(Container)?,
        Some(b'[') => decoder.deserialize_seq(Container)?,
        Some(b'-' | b'0'..=b'9') => Value::Number(Number::deserialize(&mut decoder)?),
        _ => Value::deserialize(&mut decoder)?,
    };
    decoder.end()?;
    Ok(output)
}

struct Container;
impl<'de> Visitor<'de> for Container {
    type Value = Value;
    fn expecting(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("an original JSON object or array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        let mut output = Vec::new();
        while let Some(raw) = sequence.next_element::<&RawValue>()? {
            output.push(value(raw.get().as_bytes()).map_err(de::Error::custom)?);
        }
        Ok(Value::Array(output))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut output = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if output.contains_key(&key) {
                return Err(de::Error::custom("duplicate JSON object key"));
            }
            let raw = map.next_value::<&RawValue>()?;
            output.insert(key, value(raw.get().as_bytes()).map_err(de::Error::custom)?);
        }
        Ok(Value::Object(output))
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    #[test]
    fn original_json_keeps_scalar_types_and_large_number_lexemes() {
        let bytes = br#"{"metadata":{"session":""},"items":[false,null,[],"",18446744073709551616,1.2300e+45]}"#;
        let parsed = parse(bytes).expect("unambiguous JSON");
        assert_eq!(
            parsed,
            serde_json::from_slice::<serde_json::Value>(bytes).expect("JSON")
        );
        assert_eq!(parsed["metadata"]["session"], "");
        assert_eq!(
            parsed["items"][4].as_number().expect("number").as_str(),
            "18446744073709551616"
        );
        assert_eq!(
            parsed["items"][5].as_number().expect("number").as_str(),
            "1.2300e+45"
        );
    }
    #[test]
    fn duplicate_names_are_rejected_at_every_object_depth() {
        for bytes in [
            br#"{"metadata":{"session":"a","session":"b"}}"#.as_slice(),
            br#"{"metadata":{"session":"a"},"metadata":{"session":"b"}}"#,
            br#"{"items":[{"id":"a","\u0069d":"b"}]}"#,
            br#"{"id":"same","id":"same"}"#,
            br#"{"$serde_json::private::Number":"1.0","\u0024serde_json::private::Number":"1.0"}"#,
        ] {
            assert!(
                parse(bytes)
                    .expect_err("ambiguous JSON")
                    .to_string()
                    .contains("duplicate JSON object key")
            );
        }
        assert!(parse(br#"{"id":"a"} {"id":"b"}"#).is_err());
        assert!(parse(br#"{"items":[{"id":"a"},{"id":"a"}]}"#).is_ok());
    }

    #[test]
    fn literal_private_token_maps_remain_maps_at_every_position() {
        let number_map =
            parse(br#"{"$serde_json::private::Number":"1.0"}"#).expect("literal original map");
        assert!(number_map.is_object());
        assert_eq!(number_map["$serde_json::private::Number"], "1.0");
        let nested = parse(
            br#" {"items":[{"$serde_json::private::Number":"1.0"}],"raw":{"$serde_json::private::RawValue":"false"}} "#,
        )
        .expect("nested original maps");
        assert_eq!(nested["items"][0], number_map);
        assert!(nested["raw"].is_object());
        assert_eq!(nested["raw"]["$serde_json::private::RawValue"], "false");
        assert_ne!(number_map, parse(b"1.0").expect("actual number"));
    }

    #[test]
    fn actual_numbers_keep_precision_and_the_locked_number_representation() {
        for (original, retained) in [
            ("-0", "0"),
            ("1.0", "1.0"),
            ("1e400", "1e+400"),
            ("-1e999", "-1e+999"),
            ("18446744073709551616000", "18446744073709551616000"),
        ] {
            for bytes in [original.to_owned(), format!("[{original}]")] {
                let parsed = parse(bytes.as_bytes()).expect("valid arbitrary-precision number");
                let number = parsed.as_array().map_or(&parsed, |items| &items[0]);
                assert_eq!(
                    number.as_number().expect("actual number").as_str(),
                    retained
                );
            }
        }
        for bytes in [b"NaN".as_slice(), b"Infinity", b"1.0 false", b"[1,]"] {
            assert!(parse(bytes).is_err());
        }
    }

    #[test]
    fn nesting_admission_matches_the_locked_serde_default() {
        let mut original = "0".to_owned();
        for depth in 0..160 {
            let expected = serde_json::from_str::<serde_json::Value>(&original);
            let actual = parse(original.as_bytes());
            assert_eq!(actual.is_ok(), expected.is_ok(), "nesting depth {depth}");
            if let (Err(actual), Err(expected)) = (actual, expected) {
                assert_eq!(actual.to_string(), expected.to_string());
            }
            original = if depth % 2 == 0 {
                format!("[{original}]")
            } else {
                format!("{{\"value\":{original}}}")
            };
        }
    }
}
