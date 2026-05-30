/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::{Map, Value};

pub const SENTINEL_KEY: &str = "@blob";

pub fn import_blob_ids<F>(value: &mut Value, mut resolve: F) -> Result<(), BlobWalkError>
where
    F: FnMut(&str) -> Result<i64, BlobWalkError>,
{
    walk_import(value, &mut resolve)
}

fn walk_import<F>(value: &mut Value, resolve: &mut F) -> Result<(), BlobWalkError>
where
    F: FnMut(&str) -> Result<i64, BlobWalkError>,
{
    match value {
        Value::Object(map) => {
            if let Some(Value::String(blob_id)) = map.remove("blobId") {
                let local = resolve(&blob_id)?;
                map.insert(SENTINEL_KEY.to_owned(), Value::from(local));
            }
            for child in map.values_mut() {
                walk_import(child, resolve)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_import(item, resolve)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn export_blob_ids<F>(value: &mut Value, mut resolve: F) -> Result<(), BlobWalkError>
where
    F: FnMut(i64) -> Result<String, BlobWalkError>,
{
    walk_export(value, &mut resolve)
}

fn walk_export<F>(value: &mut Value, resolve: &mut F) -> Result<(), BlobWalkError>
where
    F: FnMut(i64) -> Result<String, BlobWalkError>,
{
    match value {
        Value::Object(map) => {
            if let Some(sentinel) = map.remove(SENTINEL_KEY) {
                let local_id = sentinel.as_i64().ok_or(BlobWalkError::MalformedSentinel)?;
                let target_jmap_id = resolve(local_id)?;
                map.insert("blobId".to_owned(), Value::String(target_jmap_id));
            }
            for child in map.values_mut() {
                walk_export(child, resolve)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_export(item, resolve)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn prepend_property(target: &mut Map<String, Value>, key: &str, value: Value) {
    let existing: Vec<(String, Value)> =
        target.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    target.clear();
    target.insert(key.to_owned(), value);
    for (k, v) in existing {
        if k != key {
            target.insert(k, v);
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BlobWalkError {
    #[error("malformed @blob sentinel: expected integer local id")]
    MalformedSentinel,
    #[error("blob resolver failed: {0}")]
    Resolver(String),
}
