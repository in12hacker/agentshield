//! Fixed, nonrecursive SQL column mapping. No envelope JSON or generic JSON-node store.
use super::StoreError;
use rusqlite::{Connection, params_from_iter, types::Value as Sql};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
pub(super) type Object = Map<String, Value>;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Text,
    Int,
    Bool,
    U64,
    Bytes,
    Ref(&'static str),
}
pub(super) type Columns = &'static [(&'static str, Kind)];
pub(super) fn object(value: &impl Serialize) -> Result<Object, StoreError> {
    match serde_json::to_value(value)? {
        Value::Object(v) => Ok(v),
        _ => Err(StoreError::Invalid),
    }
}
pub(super) fn decode<T: DeserializeOwned>(value: Value) -> Result<T, StoreError> {
    Ok(serde_json::from_value(value)?)
}
pub(super) fn string(value: &impl Serialize) -> Result<String, StoreError> {
    match serde_json::to_value(value)? {
        Value::String(v) => Ok(v),
        _ => Err(StoreError::Invalid),
    }
}
pub(super) fn named<T: DeserializeOwned>(value: &str) -> Result<T, StoreError> {
    decode(Value::String(value.into()))
}
pub(super) fn number(value: u64) -> Value {
    Value::String(value.to_string())
}
pub(super) fn integer(value: i64) -> Value {
    Value::Number(value.into())
}
pub(super) fn bytes(value: &[u8]) -> Value {
    Value::Array(value.iter().map(|v| Value::Number((*v).into())).collect())
}
pub(super) fn take(value: &mut Object, name: &str) -> Result<Value, StoreError> {
    value.remove(name).ok_or(StoreError::Invalid)
}
pub(super) fn child(value: &mut Object, name: &str) -> Result<Object, StoreError> {
    match take(value, name)? {
        Value::Object(v) => Ok(v),
        _ => Err(StoreError::Invalid),
    }
}
pub(super) fn blob(value: &Value) -> Result<Vec<u8>, StoreError> {
    decode(value.clone())
}
pub(super) fn put(
    conn: &Connection,
    table: &str,
    cols: Columns,
    object: Object,
) -> Result<i64, StoreError> {
    if object.len() != cols.len() || cols.iter().any(|(name, _)| !object.contains_key(*name)) {
        return Err(StoreError::Invalid);
    }
    let mut values = Vec::with_capacity(cols.len());
    for (name, kind) in cols {
        let v = &object[*name];
        values.push(if v.is_null() {
            Sql::Null
        } else {
            match kind {
                Kind::Text => Sql::Text(v.as_str().ok_or(StoreError::Invalid)?.into()),
                Kind::Int | Kind::Ref(_) => Sql::Integer(v.as_i64().ok_or(StoreError::Invalid)?),
                Kind::Bool => Sql::Integer(i64::from(v.as_bool().ok_or(StoreError::Invalid)?)),
                Kind::U64 => Sql::Blob(
                    v.as_str()
                        .ok_or(StoreError::Invalid)?
                        .parse::<u64>()
                        .map_err(|_| StoreError::Invalid)?
                        .to_be_bytes()
                        .to_vec(),
                ),
                Kind::Bytes => Sql::Blob(blob(v)?),
            }
        });
    }
    let names = cols
        .iter()
        .map(|(n, _)| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(",");
    let marks = (1..=cols.len())
        .map(|n| format!("?{n}"))
        .collect::<Vec<_>>()
        .join(",");
    conn.execute(
        &format!("INSERT INTO {table}({names}) VALUES({marks})"),
        params_from_iter(values),
    )?;
    Ok(conn.last_insert_rowid())
}
pub(super) fn get(
    conn: &Connection,
    table: &str,
    cols: Columns,
    id: i64,
) -> Result<Object, StoreError> {
    let names = cols
        .iter()
        .map(|(n, _)| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(",");
    let values = conn.query_row(
        &format!("SELECT {names} FROM {table} WHERE id=?1"),
        [id],
        |r| {
            (0..cols.len())
                .map(|i| r.get::<_, Sql>(i))
                .collect::<Result<Vec<_>, _>>()
        },
    )?;
    let mut object = Object::new();
    for ((name, kind), v) in cols.iter().zip(values) {
        let v = match (v, kind) {
            (Sql::Null, _) => Value::Null,
            (Sql::Text(s), Kind::Text) => Value::String(s),
            (Sql::Integer(n), Kind::Int | Kind::Ref(_)) => integer(n),
            (Sql::Integer(0), Kind::Bool) => Value::Bool(false),
            (Sql::Integer(1), Kind::Bool) => Value::Bool(true),
            (Sql::Blob(b), Kind::U64) => number(u64::from_be_bytes(
                b.try_into().map_err(|_| StoreError::Invalid)?,
            )),
            (Sql::Blob(b), Kind::Bytes) => bytes(&b),
            _ => return Err(StoreError::Invalid),
        };
        object.insert((*name).into(), v);
    }
    Ok(object)
}
pub(super) fn create(conn: &Connection, table: &str, cols: Columns) -> Result<(), StoreError> {
    let declarations=cols.iter().map(|(name,kind)|match kind{
        Kind::Ref(target)=>format!("\"{name}\" INTEGER REFERENCES {target}(id)"),
        Kind::U64=>format!("\"{name}\" BLOB CHECK(\"{name}\" IS NULL OR (typeof(\"{name}\")='blob' AND length(\"{name}\")=8))"),
        Kind::Bytes=>format!("\"{name}\" BLOB"),Kind::Text=>format!("\"{name}\" TEXT"),_=>format!("\"{name}\" INTEGER"),
    }).collect::<Vec<_>>().join(",");
    conn.execute_batch(&format!(
        "CREATE TABLE IF NOT EXISTS {table}(id INTEGER PRIMARY KEY,{declarations})"
    ))?;
    Ok(())
}
