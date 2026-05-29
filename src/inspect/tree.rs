/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;
use std::io::Write;

use rusqlite::Connection;

use super::{blob_summary, format_count};
use crate::error::Error;

struct MailboxNode {
    id: i64,
    parent_id: Option<i64>,
    name: String,
    role: Option<String>,
    sort_order: i64,
    msg_count: u64,
}

pub fn write_mailboxes(conn: &Connection, out: &mut impl Write) -> Result<(), Error> {
    let nodes = load_mailboxes(conn)?;
    writeln!(out, "Mailboxes ({})", format_count(nodes.len() as u64))?;
    if nodes.is_empty() {
        writeln!(out, "(none)")?;
        return Ok(());
    }
    let by_id: HashMap<i64, &MailboxNode> = nodes.iter().map(|n| (n.id, n)).collect();
    let mut by_parent: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
    for n in &nodes {
        by_parent.entry(n.parent_id).or_default().push(n.id);
    }
    for ids in by_parent.values_mut() {
        ids.sort_by(|a, b| {
            let na = by_id[a];
            let nb = by_id[b];
            na.sort_order
                .cmp(&nb.sort_order)
                .then_with(|| na.name.cmp(&nb.name))
        });
    }
    let roots: Vec<i64> = by_parent.get(&None).cloned().unwrap_or_default();
    let last = roots.len().saturating_sub(1);
    for (i, id) in roots.iter().enumerate() {
        write_mailbox(out, &by_id, &by_parent, *id, "", i == last)?;
    }
    Ok(())
}

fn write_mailbox(
    out: &mut impl Write,
    by_id: &HashMap<i64, &MailboxNode>,
    by_parent: &HashMap<Option<i64>, Vec<i64>>,
    id: i64,
    prefix: &str,
    is_last: bool,
) -> Result<(), Error> {
    let node = by_id[&id];
    let connector = if is_last { "└── " } else { "├── " };
    let mut tags: Vec<String> = Vec::new();
    if let Some(role) = &node.role {
        tags.push(format!("role={role}"));
    }
    tags.push(format!("{} msgs", format_count(node.msg_count)));
    writeln!(
        out,
        "{prefix}{connector}{}  [{}]",
        node.name,
        tags.join(", ")
    )?;
    let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
    if let Some(children) = by_parent.get(&Some(id)) {
        let last = children.len() - 1;
        for (i, child) in children.iter().enumerate() {
            write_mailbox(out, by_id, by_parent, *child, &child_prefix, i == last)?;
        }
    }
    Ok(())
}

fn load_mailboxes(conn: &Connection) -> Result<Vec<MailboxNode>, Error> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.parent_id, m.name, m.role, m.sort_order,
                (SELECT COUNT(*) FROM emails e
                 WHERE EXISTS (SELECT 1 FROM json_each(e.mailbox_ids) j
                                WHERE CAST(j.value AS INTEGER) = m.id))
           FROM mailboxes m",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MailboxNode {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            name: r.get(2)?,
            role: r.get(3)?,
            sort_order: r.get(4)?,
            msg_count: r.get::<_, i64>(5)? as u64,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

struct FileNodeRow {
    id: i64,
    parent_id: Option<i64>,
    node_type: String,
    blob_id: Option<i64>,
    name: String,
    media_type: Option<String>,
    target: Option<String>,
    child_count: u64,
}

pub fn write_file_nodes(conn: &Connection, out: &mut impl Write) -> Result<(), Error> {
    let nodes = load_file_nodes(conn)?;
    writeln!(out, "File nodes ({})", format_count(nodes.len() as u64))?;
    if nodes.is_empty() {
        writeln!(out, "(none)")?;
        return Ok(());
    }
    let by_id: HashMap<i64, &FileNodeRow> = nodes.iter().map(|n| (n.id, n)).collect();
    let mut by_parent: HashMap<Option<i64>, Vec<i64>> = HashMap::new();
    for n in &nodes {
        by_parent.entry(n.parent_id).or_default().push(n.id);
    }
    for ids in by_parent.values_mut() {
        ids.sort_by(|a, b| {
            let na = by_id[a];
            let nb = by_id[b];
            (na.node_type != "directory")
                .cmp(&(nb.node_type != "directory"))
                .then_with(|| na.name.cmp(&nb.name))
        });
    }
    let roots: Vec<i64> = by_parent.get(&None).cloned().unwrap_or_default();
    let last = roots.len().saturating_sub(1);
    for (i, id) in roots.iter().enumerate() {
        write_file_node(conn, out, &by_id, &by_parent, *id, "", i == last)?;
    }
    Ok(())
}

fn write_file_node(
    conn: &Connection,
    out: &mut impl Write,
    by_id: &HashMap<i64, &FileNodeRow>,
    by_parent: &HashMap<Option<i64>, Vec<i64>>,
    id: i64,
    prefix: &str,
    is_last: bool,
) -> Result<(), Error> {
    let node = by_id[&id];
    let connector = if is_last { "└── " } else { "├── " };
    let display = match node.node_type.as_str() {
        "directory" => format!("{}/", node.name),
        _ => node.name.clone(),
    };
    let detail = match node.node_type.as_str() {
        "directory" => format!("({} children)", format_count(node.child_count)),
        "symlink" => {
            let target = node.target.as_deref().unwrap_or("[]");
            format!("symlink -> {target}")
        }
        _ => {
            let media = node
                .media_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            match node.blob_id {
                Some(bid) => format!("{media}, {}", blob_summary(conn, bid)?),
                None => format!("{media}, (no blob)"),
            }
        }
    };
    writeln!(out, "{prefix}{connector}{display}  {detail}")?;
    let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
    if let Some(children) = by_parent.get(&Some(id)) {
        let last = children.len() - 1;
        for (i, child) in children.iter().enumerate() {
            write_file_node(
                conn,
                out,
                by_id,
                by_parent,
                *child,
                &child_prefix,
                i == last,
            )?;
        }
    }
    Ok(())
}

fn load_file_nodes(conn: &Connection) -> Result<Vec<FileNodeRow>, Error> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.parent_id, f.node_type, f.blob_id, f.name, f.media_type, f.target,
                (SELECT COUNT(*) FROM file_nodes c WHERE c.parent_id = f.id)
           FROM file_nodes f",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(FileNodeRow {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            node_type: r.get(2)?,
            blob_id: r.get(3)?,
            name: r.get(4)?,
            media_type: r.get(5)?,
            target: r.get(6)?,
            child_count: r.get::<_, i64>(7)? as u64,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{blobs, init};
    use rusqlite::params;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init::apply_schema(&c).unwrap();
        c
    }

    fn insert_mailbox(
        c: &Connection,
        name: &str,
        parent: Option<i64>,
        role: Option<&str>,
        sort: i64,
    ) -> i64 {
        c.execute(
            "INSERT INTO mailboxes (name, parent_id, role, sort_order, is_subscribed)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![name, parent, role, sort],
        )
        .unwrap();
        c.last_insert_rowid()
    }

    fn insert_email_in(c: &Connection, mailbox_id: i64) {
        let blob = blobs::intern_blob(c, format!("seed-{mailbox_id}").as_bytes()).unwrap();
        c.execute(
            "INSERT INTO emails (blob_id, received_at, mailbox_ids, keywords)
             VALUES (?1, '2024-01-01T00:00:00Z', ?2, '[]')",
            params![blob, format!("[{mailbox_id}]")],
        )
        .unwrap();
    }

    #[test]
    fn mailbox_tree_draws_box_characters_and_msg_counts() {
        let c = mem();
        let inbox = insert_mailbox(&c, "Inbox", None, Some("inbox"), 0);
        let _archived = insert_mailbox(&c, "Archived", Some(inbox), None, 0);
        let _y2024 = insert_mailbox(&c, "2024", Some(inbox), None, 1);
        let _sent = insert_mailbox(&c, "Sent", None, Some("sent"), 1);
        insert_email_in(&c, inbox);
        insert_email_in(&c, inbox);
        let mut buf = Vec::new();
        write_mailboxes(&c, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Mailboxes (4)"), "header missing: {s}");
        assert!(s.contains("├── Inbox  [role=inbox, 2 msgs]"), "got:\n{s}");
        assert!(s.contains("│   ├── Archived"), "got:\n{s}");
        assert!(s.contains("│   └── 2024"), "got:\n{s}");
        assert!(s.contains("└── Sent  [role=sent, 0 msgs]"), "got:\n{s}");
    }

    #[test]
    fn mailbox_tree_empty_says_none() {
        let c = mem();
        let mut buf = Vec::new();
        write_mailboxes(&c, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Mailboxes (0)"));
        assert!(s.contains("(none)"));
    }

    #[test]
    fn file_node_tree_distinguishes_dirs_files_symlinks() {
        let c = mem();
        let docs: i64 = {
            c.execute(
                "INSERT INTO file_nodes (parent_id, node_type, name, created)
                 VALUES (NULL, 'directory', 'Documents', '2024-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            c.last_insert_rowid()
        };
        let blob = blobs::intern_blob(&c, b"hello world").unwrap();
        c.execute(
            "INSERT INTO file_nodes (parent_id, node_type, blob_id, name, media_type, created)
             VALUES (?1, 'file', ?2, 'note.txt', 'text/plain', '2024-01-01T00:00:00Z')",
            params![docs, blob],
        )
        .unwrap();
        c.execute(
            "INSERT INTO file_nodes (parent_id, node_type, target, name, created)
             VALUES (?1, 'symlink', '[\"..\",\"etc\"]', 'shortcut', '2024-01-01T00:00:00Z')",
            params![docs],
        )
        .unwrap();
        let mut buf = Vec::new();
        write_file_nodes(&c, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("File nodes (3)"));
        assert!(s.contains("Documents/"));
        assert!(s.contains("(2 children)"));
        assert!(s.contains("note.txt"));
        assert!(s.contains("text/plain, 11 B blake3="));
        assert!(s.contains("symlink -> [\"..\",\"etc\"]"));
    }
}
