/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};

pub fn insert(
    conn: &Connection,
    source_id: i64,
    name: &str,
    local_id: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_id_managesieve (source_id, name, local_id)
         VALUES (?1, ?2, ?3)",
        params![source_id, name, local_id],
    )?;
    Ok(())
}

pub fn local_for(
    conn: &Connection,
    source_id: i64,
    name: &str,
) -> Result<Option<i64>, rusqlite::Error> {
    conn.query_row(
        "SELECT local_id FROM sync_id_managesieve WHERE source_id = ?1 AND name = ?2",
        params![source_id, name],
        |row| row.get(0),
    )
    .optional()
}

pub fn all_names(
    conn: &Connection,
    source_id: i64,
) -> Result<HashMap<String, i64>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT name, local_id FROM sync_id_managesieve WHERE source_id = ?1")?;
    let rows = stmt.query_map(params![source_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut out = HashMap::new();
    for r in rows {
        let (n, l) = r?;
        out.insert(n, l);
    }
    Ok(out)
}

pub fn delete(conn: &Connection, source_id: i64, name: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM sync_id_managesieve WHERE source_id = ?1 AND name = ?2",
        params![source_id, name],
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
                kind: "managesieve".to_owned(),
                session_url: "sieve://host:4190".to_owned(),
                account_id: "alice".to_owned(),
            },
            None,
            "alice",
        )
        .unwrap();
        (c, sid)
    }

    #[test]
    fn insert_and_lookup_by_name() {
        let (c, sid) = setup();
        insert(&c, sid, "vacation", 7).unwrap();
        assert_eq!(local_for(&c, sid, "vacation").unwrap(), Some(7));
        assert_eq!(local_for(&c, sid, "missing").unwrap(), None);
    }

    #[test]
    fn all_names_returns_full_map() {
        let (c, sid) = setup();
        insert(&c, sid, "a", 1).unwrap();
        insert(&c, sid, "b", 2).unwrap();
        let map = all_names(&c, sid).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("a"), Some(&1));
    }

    #[test]
    fn delete_removes_row() {
        let (c, sid) = setup();
        insert(&c, sid, "a", 1).unwrap();
        delete(&c, sid, "a").unwrap();
        assert_eq!(local_for(&c, sid, "a").unwrap(), None);
    }

    #[test]
    fn re_insert_with_same_name_replaces_local_id() {
        let (c, sid) = setup();
        insert(&c, sid, "a", 1).unwrap();
        insert(&c, sid, "a", 2).unwrap();
        assert_eq!(local_for(&c, sid, "a").unwrap(), Some(2));
        assert_eq!(all_names(&c, sid).unwrap().len(), 1);
    }

    #[test]
    fn insert_or_replace_reassigns_when_local_id_clashes() {
        let (c, sid) = setup();
        insert(&c, sid, "a", 1).unwrap();
        insert(&c, sid, "b", 1).unwrap();
        assert!(local_for(&c, sid, "a").unwrap().is_none());
        assert_eq!(local_for(&c, sid, "b").unwrap(), Some(1));
    }
}
