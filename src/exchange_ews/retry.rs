/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::time::Duration;

use crate::exchange_ews::types::ResponseCode;
use crate::jmap::retry::Disposition;

pub fn classify_http_status(status: u16) -> Disposition {
    match status {
        429 | 502 | 503 | 504 => Disposition::Retryable,
        _ => Disposition::Fatal,
    }
}

#[derive(Debug, Clone)]
pub enum FaultDisposition {
    Fatal,
    Auth,
    VersionError,
    Retryable { delay: Option<Duration> },
}

pub fn classify_fault(code: &ResponseCode) -> FaultDisposition {
    match code {
        ResponseCode::ServerBusy { back_off_ms } => FaultDisposition::Retryable {
            delay: back_off_ms.map(Duration::from_millis),
        },
        ResponseCode::TimeoutExpired
        | ResponseCode::InternalServerTransientError
        | ResponseCode::MailboxStoreUnavailable
        | ResponseCode::ConnectionFailed
        | ResponseCode::AdUnavailable
        | ResponseCode::BatchProcessingStopped => FaultDisposition::Retryable { delay: None },
        ResponseCode::AuthenticationRequired => FaultDisposition::Auth,
        ResponseCode::InvalidServerVersion
        | ResponseCode::IncorrectSchemaVersion
        | ResponseCode::InvalidRequest
        | ResponseCode::InvalidSchemaVersionForMailboxVersion => FaultDisposition::VersionError,
        _ => FaultDisposition::Fatal,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemAction {
    Success,
    Vanished,
    Skip,
    Retryable,
    RetryDiscardSyncState,
}

pub fn classify_item_response(code: &ResponseCode) -> ItemAction {
    match code {
        ResponseCode::NoError => ItemAction::Success,
        ResponseCode::ItemNotFound | ResponseCode::AttachmentNotFound => ItemAction::Vanished,
        ResponseCode::ServerBusy { .. }
        | ResponseCode::TimeoutExpired
        | ResponseCode::InternalServerTransientError
        | ResponseCode::MailboxStoreUnavailable
        | ResponseCode::ConnectionFailed
        | ResponseCode::AdUnavailable
        | ResponseCode::BatchProcessingStopped => ItemAction::Retryable,
        ResponseCode::InvalidSyncStateData => ItemAction::RetryDiscardSyncState,
        ResponseCode::AccessDenied
        | ResponseCode::InvalidIdMalformed
        | ResponseCode::MimeContentConversionFailed
        | ResponseCode::ImpersonationFailed => ItemAction::Skip,
        ResponseCode::AuthenticationRequired => ItemAction::Skip,
        ResponseCode::InvalidServerVersion
        | ResponseCode::IncorrectSchemaVersion
        | ResponseCode::InvalidRequest
        | ResponseCode::InvalidSchemaVersionForMailboxVersion => ItemAction::Skip,
        ResponseCode::Other(_) => ItemAction::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_busy_with_backoff_is_retryable_with_delay() {
        let code = ResponseCode::ServerBusy {
            back_off_ms: Some(297749),
        };
        match classify_fault(&code) {
            FaultDisposition::Retryable { delay: Some(d) } => {
                assert_eq!(d, Duration::from_millis(297749));
            }
            _ => panic!("expected retryable with delay"),
        }
    }

    #[test]
    fn item_responses_are_classified() {
        assert_eq!(
            classify_item_response(&ResponseCode::NoError),
            ItemAction::Success
        );
        assert_eq!(
            classify_item_response(&ResponseCode::ItemNotFound),
            ItemAction::Vanished
        );
        assert_eq!(
            classify_item_response(&ResponseCode::AttachmentNotFound),
            ItemAction::Vanished
        );
        assert_eq!(
            classify_item_response(&ResponseCode::AccessDenied),
            ItemAction::Skip
        );
        assert_eq!(
            classify_item_response(&ResponseCode::InvalidSyncStateData),
            ItemAction::RetryDiscardSyncState
        );
        assert_eq!(
            classify_item_response(&ResponseCode::ServerBusy { back_off_ms: None }),
            ItemAction::Retryable
        );
    }

    #[test]
    fn http_status_table() {
        assert_eq!(classify_http_status(429), Disposition::Retryable);
        assert_eq!(classify_http_status(503), Disposition::Retryable);
        assert_eq!(classify_http_status(500), Disposition::Fatal);
        assert_eq!(classify_http_status(401), Disposition::Fatal);
    }

    #[test]
    fn schema_version_codes_request_a_downgrade() {
        for code in [
            ResponseCode::InvalidServerVersion,
            ResponseCode::IncorrectSchemaVersion,
            ResponseCode::InvalidRequest,
            ResponseCode::InvalidSchemaVersionForMailboxVersion,
        ] {
            assert!(matches!(
                classify_fault(&code),
                FaultDisposition::VersionError
            ));
        }
    }

    #[test]
    fn transient_codes_are_retryable_at_item_and_fault_level() {
        for code in [
            ResponseCode::InternalServerTransientError,
            ResponseCode::MailboxStoreUnavailable,
            ResponseCode::ConnectionFailed,
            ResponseCode::AdUnavailable,
            ResponseCode::BatchProcessingStopped,
        ] {
            assert_eq!(classify_item_response(&code), ItemAction::Retryable);
            assert!(matches!(
                classify_fault(&code),
                FaultDisposition::Retryable { .. }
            ));
        }
    }

    #[test]
    fn mime_content_conversion_failure_skips_item() {
        assert_eq!(
            classify_item_response(&ResponseCode::MimeContentConversionFailed),
            ItemAction::Skip
        );
    }

    #[test]
    fn other_unknown_codes_skip_at_item_level() {
        assert_eq!(
            classify_item_response(&ResponseCode::Other("ErrorWhoKnows".to_owned())),
            ItemAction::Skip
        );
    }
}
