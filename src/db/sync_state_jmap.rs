/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::ObjectType;

pub fn get(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT state FROM sync_state_jmap WHERE source_id = ?1 AND type_name = ?2",
        params![source_id, ty.jmap_name()],
        |row| row.get(0),
    )
    .optional()
}

pub fn upsert(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
    state: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_state_jmap (source_id, type_name, state)
         VALUES (?1, ?2, ?3)
         ON CONFLICT (source_id, type_name)
         DO UPDATE SET state = excluded.state",
        params![source_id, ty.jmap_name(), state],
    )?;
    Ok(())
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
                kind: "jmap".to_owned(),
                session_url: "https://host".to_owned(),
                account_id: "alice".to_owned(),
            },
            None,
            "alice",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn upsert_then_get_returns_latest_per_type() {
        let (c, sid) = setup();
        assert!(get(&c, sid, ObjectType::Email).unwrap().is_none());
        upsert(&c, sid, ObjectType::Email, "s1").unwrap();
        upsert(&c, sid, ObjectType::Mailbox, "m1").unwrap();
        assert_eq!(
            get(&c, sid, ObjectType::Email).unwrap().as_deref(),
            Some("s1")
        );
        assert_eq!(
            get(&c, sid, ObjectType::Mailbox).unwrap().as_deref(),
            Some("m1")
        );
        upsert(&c, sid, ObjectType::Email, "s2").unwrap();
        assert_eq!(
            get(&c, sid, ObjectType::Email).unwrap().as_deref(),
            Some("s2")
        );
        assert_eq!(
            get(&c, sid, ObjectType::Mailbox).unwrap().as_deref(),
            Some("m1")
        );
    }
}
