/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpClass {
    Success,
    Retryable,
    Auth,
    Vanished,
    Fatal,
}

pub fn classify_http_status(status: u16) -> HttpClass {
    match status {
        200..=299 => HttpClass::Success,
        401 => HttpClass::Auth,
        403 => HttpClass::Auth,
        404 | 410 => HttpClass::Vanished,
        429 | 500 | 502 | 503 | 504 => HttpClass::Retryable,
        _ => HttpClass::Fatal,
    }
}

pub fn is_throttled(status: u16) -> bool {
    matches!(status, 429 | 503)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_status_is_retryable() {
        assert_eq!(classify_http_status(429), HttpClass::Retryable);
        assert!(is_throttled(429));
    }

    #[test]
    fn service_unavailable_is_retryable_and_throttle_marked() {
        assert_eq!(classify_http_status(503), HttpClass::Retryable);
        assert!(is_throttled(503));
    }

    #[test]
    fn bad_gateway_and_timeout_retry() {
        assert_eq!(classify_http_status(502), HttpClass::Retryable);
        assert_eq!(classify_http_status(504), HttpClass::Retryable);
    }

    #[test]
    fn insufficient_storage_is_per_item_fatal() {
        assert_eq!(classify_http_status(507), HttpClass::Fatal);
    }

    #[test]
    fn auth_failure_is_marked_distinctly() {
        assert_eq!(classify_http_status(401), HttpClass::Auth);
        assert_eq!(classify_http_status(403), HttpClass::Auth);
    }

    #[test]
    fn not_found_and_gone_are_vanished() {
        assert_eq!(classify_http_status(404), HttpClass::Vanished);
        assert_eq!(classify_http_status(410), HttpClass::Vanished);
    }

    #[test]
    fn unknown_4xx_is_fatal() {
        assert_eq!(classify_http_status(400), HttpClass::Fatal);
        assert_eq!(classify_http_status(405), HttpClass::Fatal);
        assert_eq!(classify_http_status(408), HttpClass::Fatal);
        assert_eq!(classify_http_status(422), HttpClass::Fatal);
        assert_eq!(classify_http_status(423), HttpClass::Fatal);
    }
}
