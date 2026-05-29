/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::ObjectType;

pub fn insert(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
    jmap_id: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_id_jmap (source_id, type_name, jmap_id, local_id)
         VALUES (?1, ?2, ?3, ?4)",
        params![source_id, ty.jmap_name(), jmap_id, local_id],
    )?;
    Ok(())
}

pub fn local_for(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
    jmap_id: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_jmap
         WHERE source_id = ?1 AND type_name = ?2 AND jmap_id = ?3",
        params![source_id, ty.jmap_name(), jmap_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn jmap_to_local(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT jmap_id, local_id FROM sync_id_jmap WHERE source_id = ?1 AND type_name = ?2",
    )?;
    let rows = stmt.query_map(params![source_id, ty.jmap_name()], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for r in rows {
        let (j, l) = r?;
        map.insert(j, l);
    }
    Ok(map)
}

pub fn delete(
    conn: &Connection,
    source_id: i64,
    ty: ObjectType,
    jmap_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_jmap
         WHERE source_id = ?1 AND type_name = ?2 AND jmap_id = ?3",
        params![source_id, ty.jmap_name(), jmap_id],
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
                session_url: "u".to_owned(),
                account_id: "w".to_owned(),
            },
            None,
            "alice",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn crud_roundtrip() {
        let (c, sid) = setup();
        insert(&c, sid, ObjectType::Mailbox, "M1", 1).unwrap();
        insert(&c, sid, ObjectType::Mailbox, "M2", 2).unwrap();
        assert_eq!(
            local_for(&c, sid, ObjectType::Mailbox, "M1").unwrap(),
            Some(1)
        );
        let map = jmap_to_local(&c, sid, ObjectType::Mailbox).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("M2"), Some(&2));
        delete(&c, sid, ObjectType::Mailbox, "M1").unwrap();
        assert_eq!(local_for(&c, sid, ObjectType::Mailbox, "M1").unwrap(), None);
    }
}
