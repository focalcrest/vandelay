/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::fmt;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ItemId {
    pub id: String,
    pub change_key: String,
}

impl ItemId {
    pub fn new(id: impl Into<String>, change_key: impl Into<String>) -> ItemId {
        ItemId {
            id: id.into(),
            change_key: change_key.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FolderId {
    pub id: String,
    pub change_key: String,
}

impl FolderId {
    pub fn new(id: impl Into<String>, change_key: impl Into<String>) -> FolderId {
        FolderId {
            id: id.into(),
            change_key: change_key.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailboxKind {
    Primary,
    Archive,
    PublicFolders,
}

impl MailboxKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MailboxKind::Primary => "primary",
            MailboxKind::Archive => "archive",
            MailboxKind::PublicFolders => "public-folders",
        }
    }

    pub fn distinguished_root(self) -> DistinguishedFolderId {
        match self {
            MailboxKind::Primary => DistinguishedFolderId::MsgFolderRoot,
            MailboxKind::Archive => DistinguishedFolderId::ArchiveRoot,
            MailboxKind::PublicFolders => DistinguishedFolderId::PublicFoldersRoot,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistinguishedFolderId {
    MsgFolderRoot,
    ArchiveRoot,
    PublicFoldersRoot,
    Inbox,
    SentItems,
    Drafts,
    DeletedItems,
    JunkEmail,
    Outbox,
    ConversationHistory,
    Archive,
}

impl DistinguishedFolderId {
    pub fn as_str(self) -> &'static str {
        match self {
            DistinguishedFolderId::MsgFolderRoot => "msgfolderroot",
            DistinguishedFolderId::ArchiveRoot => "archiveroot",
            DistinguishedFolderId::PublicFoldersRoot => "publicfoldersroot",
            DistinguishedFolderId::Inbox => "inbox",
            DistinguishedFolderId::SentItems => "sentitems",
            DistinguishedFolderId::Drafts => "drafts",
            DistinguishedFolderId::DeletedItems => "deleteditems",
            DistinguishedFolderId::JunkEmail => "junkemail",
            DistinguishedFolderId::Outbox => "outbox",
            DistinguishedFolderId::ConversationHistory => "conversationhistory",
            DistinguishedFolderId::Archive => "archive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerVersion {
    Exchange2007,
    Exchange2010,
    Exchange2010Sp1,
    Exchange2010Sp2,
    Exchange2013,
    Exchange2013Sp1,
    Exchange2016,
    Exchange2019,
}

impl ServerVersion {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerVersion::Exchange2007 => "Exchange2007",
            ServerVersion::Exchange2010 => "Exchange2010",
            ServerVersion::Exchange2010Sp1 => "Exchange2010_SP1",
            ServerVersion::Exchange2010Sp2 => "Exchange2010_SP2",
            ServerVersion::Exchange2013 => "Exchange2013",
            ServerVersion::Exchange2013Sp1 => "Exchange2013_SP1",
            ServerVersion::Exchange2016 => "Exchange2016",
            ServerVersion::Exchange2019 => "Exchange2019",
        }
    }

    pub fn from_build(major: u32, minor: u32) -> ServerVersion {
        match (major, minor) {
            (8, _) => ServerVersion::Exchange2007,
            (14, 0) => ServerVersion::Exchange2010,
            (14, 1) => ServerVersion::Exchange2010Sp1,
            (14, 2 | 3) => ServerVersion::Exchange2010Sp2,
            (15, 0) => ServerVersion::Exchange2013,
            (15, 1) => ServerVersion::Exchange2016,
            (15, 2) => ServerVersion::Exchange2019,
            _ if major >= 15 => ServerVersion::Exchange2019,
            _ => ServerVersion::Exchange2016,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderClass {
    Mail,
    Calendar,
    Contacts,
    Skipped,
}

impl FolderClass {
    pub fn from_ipf(class: &str) -> FolderClass {
        if class.eq_ignore_ascii_case("IPF.Note") {
            FolderClass::Mail
        } else if class.eq_ignore_ascii_case("IPF.Appointment")
            || class.starts_with("IPF.Appointment.")
        {
            FolderClass::Calendar
        } else if class.eq_ignore_ascii_case("IPF.Contact") || class.starts_with("IPF.Contact.") {
            FolderClass::Contacts
        } else {
            FolderClass::Skipped
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarItemType {
    Single,
    RecurringMaster,
    Occurrence,
    Exception,
}

impl CalendarItemType {
    pub fn parse(value: &str) -> Option<CalendarItemType> {
        match value {
            "Single" => Some(CalendarItemType::Single),
            "RecurringMaster" => Some(CalendarItemType::RecurringMaster),
            "Occurrence" => Some(CalendarItemType::Occurrence),
            "Exception" => Some(CalendarItemType::Exception),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseCode {
    NoError,
    ItemNotFound,
    AccessDenied,
    InvalidIdMalformed,
    ServerBusy { back_off_ms: Option<u64> },
    TimeoutExpired,
    InvalidSyncStateData,
    MimeContentConversionFailed,
    AttachmentNotFound,
    AuthenticationRequired,
    ImpersonationFailed,
    BatchProcessingStopped,
    InternalServerTransientError,
    MailboxStoreUnavailable,
    ConnectionFailed,
    AdUnavailable,
    Other(String),
}

impl ResponseCode {
    pub fn parse(code: &str) -> ResponseCode {
        match code {
            "NoError" => ResponseCode::NoError,
            "ErrorItemNotFound" => ResponseCode::ItemNotFound,
            "ErrorAccessDenied" => ResponseCode::AccessDenied,
            "ErrorInvalidIdMalformed" => ResponseCode::InvalidIdMalformed,
            "ErrorServerBusy" => ResponseCode::ServerBusy { back_off_ms: None },
            "ErrorTimeoutExpired" => ResponseCode::TimeoutExpired,
            "ErrorInvalidSyncStateData" => ResponseCode::InvalidSyncStateData,
            "ErrorMimeContentConversionFailed" => ResponseCode::MimeContentConversionFailed,
            "ErrorAttachmentNotFound" => ResponseCode::AttachmentNotFound,
            "ErrorAuthenticationRequired" => ResponseCode::AuthenticationRequired,
            "ErrorImpersonationFailed" => ResponseCode::ImpersonationFailed,
            "ErrorBatchProcessingStopped" => ResponseCode::BatchProcessingStopped,
            "ErrorInternalServerTransientError" => ResponseCode::InternalServerTransientError,
            "ErrorMailboxStoreUnavailable" => ResponseCode::MailboxStoreUnavailable,
            "ErrorConnectionFailed" => ResponseCode::ConnectionFailed,
            "ErrorADUnavailable" => ResponseCode::AdUnavailable,
            other => ResponseCode::Other(other.to_owned()),
        }
    }

    pub fn as_wire(&self) -> &str {
        match self {
            ResponseCode::NoError => "NoError",
            ResponseCode::ItemNotFound => "ErrorItemNotFound",
            ResponseCode::AccessDenied => "ErrorAccessDenied",
            ResponseCode::InvalidIdMalformed => "ErrorInvalidIdMalformed",
            ResponseCode::ServerBusy { .. } => "ErrorServerBusy",
            ResponseCode::TimeoutExpired => "ErrorTimeoutExpired",
            ResponseCode::InvalidSyncStateData => "ErrorInvalidSyncStateData",
            ResponseCode::MimeContentConversionFailed => "ErrorMimeContentConversionFailed",
            ResponseCode::AttachmentNotFound => "ErrorAttachmentNotFound",
            ResponseCode::AuthenticationRequired => "ErrorAuthenticationRequired",
            ResponseCode::ImpersonationFailed => "ErrorImpersonationFailed",
            ResponseCode::BatchProcessingStopped => "ErrorBatchProcessingStopped",
            ResponseCode::InternalServerTransientError => "ErrorInternalServerTransientError",
            ResponseCode::MailboxStoreUnavailable => "ErrorMailboxStoreUnavailable",
            ResponseCode::ConnectionFailed => "ErrorConnectionFailed",
            ResponseCode::AdUnavailable => "ErrorADUnavailable",
            ResponseCode::Other(s) => s.as_str(),
        }
    }
}

impl fmt::Display for ResponseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_class_mapping() {
        assert_eq!(FolderClass::from_ipf("IPF.Note"), FolderClass::Mail);
        assert_eq!(
            FolderClass::from_ipf("IPF.Appointment"),
            FolderClass::Calendar
        );
        assert_eq!(
            FolderClass::from_ipf("IPF.Appointment.Birthday"),
            FolderClass::Calendar
        );
        assert_eq!(FolderClass::from_ipf("IPF.Contact"), FolderClass::Contacts);
        assert_eq!(FolderClass::from_ipf("IPF.Task"), FolderClass::Skipped);
    }

    #[test]
    fn server_version_from_build() {
        assert_eq!(
            ServerVersion::from_build(15, 0),
            ServerVersion::Exchange2013
        );
        assert_eq!(
            ServerVersion::from_build(15, 1),
            ServerVersion::Exchange2016
        );
        assert_eq!(
            ServerVersion::from_build(15, 2),
            ServerVersion::Exchange2019
        );
        assert_eq!(
            ServerVersion::from_build(99, 99),
            ServerVersion::Exchange2019
        );
    }

    #[test]
    fn response_code_round_trip() {
        let codes = [
            "NoError",
            "ErrorItemNotFound",
            "ErrorAccessDenied",
            "ErrorInvalidIdMalformed",
            "ErrorServerBusy",
            "ErrorTimeoutExpired",
            "ErrorInvalidSyncStateData",
            "ErrorMimeContentConversionFailed",
            "ErrorAttachmentNotFound",
            "ErrorAuthenticationRequired",
            "ErrorImpersonationFailed",
            "ErrorWhatever",
        ];
        for c in codes {
            assert_eq!(ResponseCode::parse(c).as_wire(), c);
        }
    }
}
