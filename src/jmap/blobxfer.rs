/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::Value;

use crate::jmap::error::JmapError;
use crate::jmap::http::HttpClient;
use crate::jmap::session::Session;
use crate::jmap::wire::JmapId;

pub fn upload_bytes(
    client: &HttpClient,
    session: &Session,
    account_id: &str,
    content_type: &str,
    bytes: &[u8],
) -> Result<JmapId, JmapError> {
    let url = session.upload_url_for(account_id);
    let response = client.upload(&url, content_type, bytes)?;
    response
        .get("blobId")
        .and_then(Value::as_str)
        .map(|s| JmapId(s.to_owned()))
        .ok_or_else(|| JmapError::malformed("upload response has no blobId"))
}

pub fn download_bytes(
    client: &HttpClient,
    session: &Session,
    account_id: &str,
    blob_id: &str,
    type_hint: &str,
    name: &str,
) -> Result<Vec<u8>, JmapError> {
    let url = session.download_url_for(account_id, blob_id, type_hint, name);
    client.download(&url)
}
