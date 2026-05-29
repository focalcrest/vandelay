/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::Value;

use crate::jmap::retry::Disposition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DavOutcome {
    Success,
    Retryable,
    Vanished,
    Auth,
    Fatal,
}

pub fn classify(status: u16, body: &[u8]) -> DavOutcome {
    if (200..300).contains(&status) {
        return DavOutcome::Success;
    }
    if status == 401 {
        return DavOutcome::Auth;
    }
    if status == 403 {
        if is_google_usage_limit(body) {
            return DavOutcome::Retryable;
        }
        return DavOutcome::Auth;
    }
    if status == 404 || status == 410 {
        return DavOutcome::Vanished;
    }
    if matches!(status, 423 | 429 | 502 | 503 | 504 | 507) {
        return DavOutcome::Retryable;
    }
    match crate::jmap::retry::classify_http_status(status) {
        Disposition::Retryable => DavOutcome::Retryable,
        Disposition::Fatal => DavOutcome::Fatal,
    }
}

fn is_google_usage_limit(body: &[u8]) -> bool {
    let value: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let errors = value
        .get("error")
        .and_then(|e| e.get("errors"))
        .and_then(Value::as_array);
    let Some(errors) = errors else {
        return false;
    };
    errors.iter().any(|entry| {
        entry
            .get("domain")
            .and_then(Value::as_str)
            .map(|d| d == "usageLimits")
            .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_range() {
        assert_eq!(classify(200, b""), DavOutcome::Success);
        assert_eq!(classify(207, b""), DavOutcome::Success);
        assert_eq!(classify(299, b""), DavOutcome::Success);
    }

    #[test]
    fn auth_status() {
        assert_eq!(classify(401, b""), DavOutcome::Auth);
        assert_eq!(classify(403, b""), DavOutcome::Auth);
    }

    #[test]
    fn vanished_status() {
        assert_eq!(classify(404, b""), DavOutcome::Vanished);
        assert_eq!(classify(410, b""), DavOutcome::Vanished);
    }

    #[test]
    fn locked_and_quota_are_retryable() {
        assert_eq!(classify(423, b""), DavOutcome::Retryable);
        assert_eq!(classify(429, b""), DavOutcome::Retryable);
        assert_eq!(classify(503, b""), DavOutcome::Retryable);
        assert_eq!(classify(507, b""), DavOutcome::Retryable);
    }

    #[test]
    fn google_usage_limit_403_is_retryable() {
        let body =
            br#"{"error":{"errors":[{"domain":"usageLimits","reason":"rateLimitExceeded"}]}}"#;
        assert_eq!(classify(403, body), DavOutcome::Retryable);
    }

    #[test]
    fn non_google_403_is_auth() {
        let body = br#"{"error":{"errors":[{"domain":"global","reason":"forbidden"}]}}"#;
        assert_eq!(classify(403, body), DavOutcome::Auth);
    }

    #[test]
    fn fatal_4xx_other() {
        assert_eq!(classify(400, b""), DavOutcome::Fatal);
        assert_eq!(classify(405, b""), DavOutcome::Fatal);
        assert_eq!(classify(412, b""), DavOutcome::Fatal);
    }
}
