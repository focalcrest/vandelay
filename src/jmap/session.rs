/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Error;
use crate::jmap::error::JmapError;
use crate::jmap::http::HttpClient;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub api_url: String,
    pub upload_url: String,
    pub download_url: String,
    pub capabilities: IndexMap<String, Value>,
    pub accounts: IndexMap<String, Account>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    #[serde(default)]
    pub account_capabilities: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_objects_in_get: u64,
    pub max_objects_in_set: u64,
    pub max_calls_in_request: u64,
    pub max_concurrent_requests: u64,
    pub max_concurrent_upload: u64,
    pub max_size_request: u64,
    pub max_size_upload: u64,
}

fn parse_session(body: &str) -> Option<Session> {
    let value: Value = serde_json::from_str(body).ok()?;
    let has_shape = value.get("apiUrl").is_some()
        && value.get("accounts").is_some()
        && value.get("capabilities").is_some();
    if !has_shape {
        return None;
    }
    serde_json::from_value(value).ok()
}

fn well_known_url(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    format!("{trimmed}/.well-known/jmap")
}

impl Session {
    pub fn discover(client: &HttpClient, url: &str) -> Result<Session, JmapError> {
        match client.get(url) {
            Ok(direct) => {
                if let Some(session) = parse_session(&direct) {
                    return session.ensure_authenticated();
                }
            }
            Err(JmapError::Auth(m)) => return Err(JmapError::Auth(m)),
            Err(_) => {}
        }
        let well_known = well_known_url(url);
        match client.get(&well_known) {
            Ok(body) => {
                if let Some(session) = parse_session(&body) {
                    return session.ensure_authenticated();
                }
                Err(JmapError::Connect(format!(
                    "no JMAP Session object at {url} or {well_known}"
                )))
            }
            Err(JmapError::Auth(m)) => Err(JmapError::Auth(m)),
            Err(e) => Err(JmapError::Connect(format!(
                "session discovery failed: no Session at {url}; {well_known}: {e}"
            ))),
        }
    }

    fn ensure_authenticated(self) -> Result<Session, JmapError> {
        if self.accounts.is_empty() {
            return Err(JmapError::Auth(
                "session enumerates no accounts (anonymous session: authentication failed)"
                    .to_owned(),
            ));
        }
        Ok(self)
    }

    pub fn core_limits(&self) -> Result<Limits, Error> {
        let core = self
            .capabilities
            .get("urn:ietf:params:jmap:core")
            .ok_or_else(|| {
                Error::Connection("session has no urn:ietf:params:jmap:core capability".to_owned())
            })?;
        serde_json::from_value(core.clone())
            .map_err(|e| Error::Connection(format!("session core capability is malformed: {e}")))
    }

    pub fn account(&self, account_id: &str) -> Option<&Account> {
        self.accounts.get(account_id)
    }

    pub fn account_capabilities(&self, account_id: &str) -> Option<&IndexMap<String, Value>> {
        self.accounts
            .get(account_id)
            .map(|a| &a.account_capabilities)
    }

    pub fn supports(&self, account_id: &str, capability_urn: &str) -> bool {
        self.account_capabilities(account_id)
            .map(|caps| caps.contains_key(capability_urn))
            .unwrap_or(false)
    }

    pub fn upload_url_for(&self, account_id: &str) -> String {
        self.upload_url
            .replace("{accountId}", &encode_segment(account_id))
    }

    pub fn download_url_for(
        &self,
        account_id: &str,
        blob_id: &str,
        type_hint: &str,
        name: &str,
    ) -> String {
        self.download_url
            .replace("{accountId}", &encode_segment(account_id))
            .replace("{blobId}", &encode_segment(blob_id))
            .replace("{type}", &encode_segment(type_hint))
            .replace("{name}", &encode_segment(name))
    }
}

fn encode_segment(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let b = *byte;
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0F) as usize] as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_session() -> &'static str {
        r#"{
            "apiUrl": "https://example.org/jmap/api",
            "uploadUrl": "https://example.org/jmap/upload/{accountId}/",
            "downloadUrl": "https://example.org/jmap/download/{accountId}/{blobId}/{type}/{name}",
            "capabilities": {
                "urn:ietf:params:jmap:core": {
                    "maxObjectsInGet": 500,
                    "maxObjectsInSet": 500,
                    "maxCallsInRequest": 16,
                    "maxConcurrentRequests": 4,
                    "maxConcurrentUpload": 4,
                    "maxSizeRequest": 10000000,
                    "maxSizeUpload": 50000000
                }
            },
            "accounts": {
                "w": {
                    "name": "vspec-user@example.org",
                    "accountCapabilities": { "urn:ietf:params:jmap:mail": {} }
                }
            }
        }"#
    }

    #[test]
    fn parses_a_session_and_reads_limits() {
        let session = parse_session(raw_session()).unwrap();
        let limits = session.core_limits().unwrap();
        assert_eq!(limits.max_objects_in_get, 500);
        assert_eq!(limits.max_size_upload, 50000000);
        assert_eq!(session.accounts["w"].name, "vspec-user@example.org");
    }

    #[test]
    fn capability_gate_reads_account_capabilities() {
        let session = parse_session(raw_session()).unwrap();
        assert!(session.supports("w", "urn:ietf:params:jmap:mail"));
        assert!(!session.supports("w", "urn:ietf:params:jmap:filenode"));
        assert!(!session.supports("missing", "urn:ietf:params:jmap:mail"));
    }

    #[test]
    fn templates_upload_and_download_urls_with_encoding() {
        let session = parse_session(raw_session()).unwrap();
        assert_eq!(
            session.upload_url_for("a c"),
            "https://example.org/jmap/upload/a%20c/"
        );
        assert_eq!(
            session.download_url_for("w", "G1/2", "application/sieve", "my script"),
            "https://example.org/jmap/download/w/G1%2F2/application%2Fsieve/my%20script"
        );
    }

    #[test]
    fn rejects_body_that_is_not_a_session() {
        assert!(parse_session("{\"hello\":true}").is_none());
        assert!(parse_session("not json").is_none());
    }

    #[test]
    fn anonymous_session_is_auth_failure() {
        let raw =
            r#"{"apiUrl":"x","uploadUrl":"u","downloadUrl":"d","capabilities":{},"accounts":{}}"#;
        let session = parse_session(raw).unwrap();
        assert!(matches!(
            session.ensure_authenticated(),
            Err(JmapError::Auth(_))
        ));
    }

    #[test]
    fn well_known_appends_correctly() {
        assert_eq!(
            well_known_url("https://h.example/"),
            "https://h.example/.well-known/jmap"
        );
        assert_eq!(
            well_known_url("https://h.example"),
            "https://h.example/.well-known/jmap"
        );
    }
}
