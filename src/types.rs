/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectType {
    Mailbox,
    Email,
    Identity,
    SieveScript,
    AddressBook,
    ContactCard,
    Calendar,
    CalendarEvent,
    ParticipantIdentity,
    FileNode,
}

impl ObjectType {
    pub const ALL: [ObjectType; 10] = [
        ObjectType::Mailbox,
        ObjectType::Email,
        ObjectType::Identity,
        ObjectType::SieveScript,
        ObjectType::AddressBook,
        ObjectType::ContactCard,
        ObjectType::Calendar,
        ObjectType::CalendarEvent,
        ObjectType::ParticipantIdentity,
        ObjectType::FileNode,
    ];

    pub fn token(self) -> &'static str {
        match self {
            ObjectType::Mailbox => "mailbox",
            ObjectType::Email => "email",
            ObjectType::Identity => "identity",
            ObjectType::SieveScript => "sievescript",
            ObjectType::AddressBook => "addressbook",
            ObjectType::ContactCard => "contactcard",
            ObjectType::Calendar => "calendar",
            ObjectType::CalendarEvent => "calendarevent",
            ObjectType::ParticipantIdentity => "participantidentity",
            ObjectType::FileNode => "filenode",
        }
    }

    pub fn jmap_name(self) -> &'static str {
        match self {
            ObjectType::Mailbox => "Mailbox",
            ObjectType::Email => "Email",
            ObjectType::Identity => "Identity",
            ObjectType::SieveScript => "SieveScript",
            ObjectType::AddressBook => "AddressBook",
            ObjectType::ContactCard => "ContactCard",
            ObjectType::Calendar => "Calendar",
            ObjectType::CalendarEvent => "CalendarEvent",
            ObjectType::ParticipantIdentity => "ParticipantIdentity",
            ObjectType::FileNode => "FileNode",
        }
    }

    pub fn capability_urn(self) -> &'static str {
        match self {
            ObjectType::Mailbox | ObjectType::Email | ObjectType::Identity => {
                "urn:ietf:params:jmap:mail"
            }
            ObjectType::SieveScript => "urn:ietf:params:jmap:sieve",
            ObjectType::AddressBook | ObjectType::ContactCard => "urn:ietf:params:jmap:contacts",
            ObjectType::Calendar | ObjectType::CalendarEvent | ObjectType::ParticipantIdentity => {
                "urn:ietf:params:jmap:calendars"
            }
            ObjectType::FileNode => "urn:ietf:params:jmap:filenode",
        }
    }

    pub fn parse(token: &str) -> Result<ObjectType, Error> {
        let normalized = token.trim().to_ascii_lowercase();
        ObjectType::ALL
            .into_iter()
            .find(|t| t.token() == normalized)
            .ok_or_else(|| Error::Usage(format!("unknown object type: {token}")))
    }
}

pub fn parse_object_list(list: &str) -> Result<Vec<ObjectType>, Error> {
    let mut out = Vec::new();
    for token in list.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let parsed = ObjectType::parse(token)?;
        if !out.contains(&parsed) {
            out.push(parsed);
        }
    }
    if out.is_empty() {
        return Err(Error::Usage(
            "--objects given but resolved to an empty type list".to_owned(),
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip() {
        for t in ObjectType::ALL {
            assert_eq!(ObjectType::parse(t.token()).unwrap(), t);
        }
    }

    #[test]
    fn parse_list_dedups_and_trims() {
        let parsed = parse_object_list(" mailbox, email ,mailbox").unwrap();
        assert_eq!(parsed, vec![ObjectType::Mailbox, ObjectType::Email]);
    }

    #[test]
    fn parse_list_case_insensitive() {
        let parsed = parse_object_list("Mailbox,EMAIL").unwrap();
        assert_eq!(parsed, vec![ObjectType::Mailbox, ObjectType::Email]);
    }

    #[test]
    fn parse_list_rejects_unknown() {
        assert!(parse_object_list("mailbox,bogus").is_err());
    }

    #[test]
    fn parse_list_rejects_empty() {
        assert!(parse_object_list("  ,  ").is_err());
    }
}
