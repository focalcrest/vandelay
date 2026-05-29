/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    Primary,
    Archive,
}

impl MailboxKind {
    pub fn parse(value: &str) -> Result<MailboxKind, Error> {
        match value {
            "primary" => Ok(MailboxKind::Primary),
            "archive" => Ok(MailboxKind::Archive),
            "public-folders" => Err(Error::Usage(
                "Microsoft Graph does not expose public folders. Run `vandelay import exchange-ews --mailbox-kind public-folders` instead.".to_owned(),
            )),
            other => Err(Error::Usage(format!(
                "--mailbox-kind must be primary | archive, got {other:?}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MailboxKind::Primary => "primary",
            MailboxKind::Archive => "archive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventBodyFormat {
    Text,
    Html,
}

impl EventBodyFormat {
    pub fn parse(value: &str) -> Result<EventBodyFormat, Error> {
        match value {
            "text" => Ok(EventBodyFormat::Text),
            "html" => Ok(EventBodyFormat::Html),
            other => Err(Error::Usage(format!(
                "--event-body-format must be text | html, got {other:?}"
            ))),
        }
    }

    pub fn prefer_value(self) -> &'static str {
        match self {
            EventBodyFormat::Text => "outlook.body-content-type=\"text\"",
            EventBodyFormat::Html => "outlook.body-content-type=\"html\"",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPrincipal {
    pub id: String,
    pub user_principal_name: String,
}

pub fn synthetic_account_id(directory_id: &str, kind: MailboxKind) -> String {
    match kind {
        MailboxKind::Primary => directory_id.to_owned(),
        MailboxKind::Archive => format!("{directory_id}#archive"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_primary_and_archive() {
        assert_eq!(MailboxKind::parse("primary").unwrap(), MailboxKind::Primary);
        assert_eq!(MailboxKind::parse("archive").unwrap(), MailboxKind::Archive);
    }

    #[test]
    fn public_folders_is_redirected_to_ews() {
        let err = MailboxKind::parse("public-folders").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("public folders"));
        assert!(msg.contains("exchange-ews"));
    }

    #[test]
    fn synthetic_id_matches_ews_pattern() {
        assert_eq!(
            synthetic_account_id("u-uuid", MailboxKind::Primary),
            "u-uuid"
        );
        assert_eq!(
            synthetic_account_id("u-uuid", MailboxKind::Archive),
            "u-uuid#archive"
        );
    }

    #[test]
    fn body_format_prefer_value() {
        assert_eq!(
            EventBodyFormat::Text.prefer_value(),
            "outlook.body-content-type=\"text\""
        );
        assert_eq!(
            EventBodyFormat::Html.prefer_value(),
            "outlook.body-content-type=\"html\""
        );
    }
}
