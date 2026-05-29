/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, OptionalExtension, params};

const MAILBOX: &str = "mailbox";
const EMAIL: &str = "email";

pub fn insert_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_id_maildir
         (source_id, type_name, folder, unique_id, local_id)
         VALUES (?1, ?2, ?3, '', ?4)",
        params![source_id, MAILBOX, folder, local_id],
    )?;
    Ok(())
}

pub fn insert_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    unique_id: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_id_maildir
         (source_id, type_name, folder, unique_id, local_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![source_id, EMAIL, folder, unique_id, local_id],
    )?;
    Ok(())
}

pub fn local_for_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3 AND unique_id = ''",
        params![source_id, MAILBOX, folder],
        |row| row.get(0),
    )
    .optional()
}

pub fn local_for_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    unique_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3 AND unique_id = ?4",
        params![source_id, EMAIL, folder, unique_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn mailbox_folders(
    conn: &Connection,
    source_id: i64,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT folder, local_id FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND unique_id = ''",
    )?;
    let rows = stmt.query_map(params![source_id, MAILBOX], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (f, l) = r?;
        map.insert(f, l);
    }
    Ok(map)
}

pub fn email_ids_in_folder(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT unique_id, local_id FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL, folder], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (u, l) = r?;
        map.insert(u, l);
    }
    Ok(map)
}

pub fn delete_mailbox(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3 AND unique_id = ''",
        params![source_id, MAILBOX, folder],
    )?;
    Ok(())
}

pub fn delete_email(
    conn: &Connection,
    source_id: i64,
    folder: &str,
    unique_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3 AND unique_id = ?4",
        params![source_id, EMAIL, folder, unique_id],
    )?;
    Ok(())
}

pub fn delete_all_emails_in_folder(
    conn: &Connection,
    source_id: i64,
    folder: &str,
) -> Result<usize, rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2 AND folder = ?3",
        params![source_id, EMAIL, folder],
    )
}

pub fn folders_with_emails(
    conn: &Connection,
    source_id: i64,
) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT folder FROM sync_id_maildir
         WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, EMAIL], |row| row.get::<_, String>(0))?;
    let mut out = HashSet::new();
    for r in rows {
        out.insert(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init;
    use crate::db::sources::{SourceKey, upsert_source};

    fn setup() -> (Connection, i64) {
        let c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();
        let sid = upsert_source(
            &c,
            &SourceKey {
                kind: "maildir".to_owned(),
                session_url: "file:///home/alice/Maildir".to_owned(),
                account_id: "/home/alice/Maildir".to_owned(),
            },
            Some("Maildir"),
            "",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn mailbox_roundtrip() {
        let (c, sid) = setup();
        insert_mailbox(&c, sid, "INBOX", 7).unwrap();
        insert_mailbox(&c, sid, "Archive.2025", 8).unwrap();
        assert_eq!(local_for_mailbox(&c, sid, "INBOX").unwrap(), Some(7));
        let all = mailbox_folders(&c, sid).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("Archive.2025"), Some(&8));
        delete_mailbox(&c, sid, "INBOX").unwrap();
        assert_eq!(local_for_mailbox(&c, sid, "INBOX").unwrap(), None);
    }

    #[test]
    fn email_roundtrip() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", "1739471123.M001.host", 1).unwrap();
        insert_email(&c, sid, "INBOX", "1739471200.M002.host", 2).unwrap();
        insert_email(&c, sid, "Sent", "1739471300.M003.host", 3).unwrap();
        assert_eq!(
            local_for_email(&c, sid, "INBOX", "1739471123.M001.host").unwrap(),
            Some(1)
        );
        let inbox = email_ids_in_folder(&c, sid, "INBOX").unwrap();
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox.get("1739471123.M001.host"), Some(&1));
        let folders = folders_with_emails(&c, sid).unwrap();
        assert!(folders.contains("INBOX"));
        assert!(folders.contains("Sent"));
    }

    #[test]
    fn delete_email_removes_only_that_row() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", "u1", 1).unwrap();
        insert_email(&c, sid, "INBOX", "u2", 2).unwrap();
        delete_email(&c, sid, "INBOX", "u1").unwrap();
        let inbox = email_ids_in_folder(&c, sid, "INBOX").unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(inbox.contains_key("u2"));
    }

    #[test]
    fn delete_all_emails_in_folder_wipes_only_that_folder() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", "u1", 1).unwrap();
        insert_email(&c, sid, "INBOX", "u2", 2).unwrap();
        insert_email(&c, sid, "Sent", "u3", 3).unwrap();
        let removed = delete_all_emails_in_folder(&c, sid, "INBOX").unwrap();
        assert_eq!(removed, 2);
        assert!(email_ids_in_folder(&c, sid, "INBOX").unwrap().is_empty());
        assert_eq!(email_ids_in_folder(&c, sid, "Sent").unwrap().len(), 1);
    }

    #[test]
    fn mailbox_and_email_namespaces_do_not_collide_on_local_id() {
        let (c, sid) = setup();
        insert_mailbox(&c, sid, "INBOX", 42).unwrap();
        insert_email(&c, sid, "INBOX", "u1", 42).unwrap();
    }

    #[test]
    fn re_insert_with_same_unique_id_is_rejected_by_pk() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", "u1", 1).unwrap();
        let err = insert_email(&c, sid, "INBOX", "u1", 2).expect_err("PK conflict");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
        assert_eq!(local_for_email(&c, sid, "INBOX", "u1").unwrap(), Some(1));
        assert_eq!(email_ids_in_folder(&c, sid, "INBOX").unwrap().len(), 1);
    }

    #[test]
    fn re_insert_with_same_local_id_is_rejected_by_unique_constraint() {
        let (c, sid) = setup();
        insert_email(&c, sid, "INBOX", "u1", 42).unwrap();
        let err = insert_email(&c, sid, "INBOX", "u2", 42).expect_err("UNIQUE conflict");
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }
}
