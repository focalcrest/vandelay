/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::db::exchange_ews_ids;
use crate::error::Error;
use crate::exchange_ews::EwsClient;
use crate::exchange_ews::error::EwsError;
use crate::exchange_ews::parse::{
    FolderEntry, parse_find_folder_response, parse_folder_inner, parse_get_folder_response,
    parse_response_messages,
};
use crate::exchange_ews::types::{DistinguishedFolderId, FolderClass, MailboxKind};
use crate::exchange_ews::xml::{
    FolderRef, FolderShape, Traversal, find_folder_body, get_folder_body,
};
use crate::logging::{LEVEL_PROGRESS, Logger};
use crate::sync::TypeCounts;

pub struct FolderPlan {
    pub mail: Vec<ClassifiedFolder>,
    pub calendar: Vec<ClassifiedFolder>,
    pub contacts: Vec<ClassifiedFolder>,
    pub well_known_roles: HashMap<String, &'static str>,
    pub root_folder_id: String,
}

#[derive(Debug, Clone)]
pub struct ClassifiedFolder {
    pub folder: FolderEntry,
    pub kind: FolderClass,
}

pub fn plan_folders(
    client: &EwsClient,
    url: &str,
    mailbox_kind: MailboxKind,
    logger: Logger,
) -> Result<FolderPlan, EwsError> {
    let root = mailbox_kind.distinguished_root();
    let request = find_folder_body(FolderRef::Distinguished(root), Traversal::Deep);
    let resp = client.call(url, "FindFolder", &request)?;
    let parsed = parse_find_folder_response(&resp.body)?;
    if logger.enabled(LEVEL_PROGRESS) {
        eprintln!("FindFolder returned {} folders", parsed.folders.len());
    }
    let mut mail = Vec::new();
    let mut calendar = Vec::new();
    let mut contacts = Vec::new();
    for folder in parsed.folders {
        let class = FolderClass::from_ipf(&folder.folder_class);
        match (mailbox_kind, class) {
            (MailboxKind::Archive, FolderClass::Mail) => {
                mail.push(ClassifiedFolder {
                    folder,
                    kind: class,
                });
            }
            (MailboxKind::Archive, _) => {}
            (_, FolderClass::Mail) => mail.push(ClassifiedFolder {
                folder,
                kind: class,
            }),
            (_, FolderClass::Calendar) => {
                calendar.push(ClassifiedFolder {
                    folder,
                    kind: class,
                });
            }
            (_, FolderClass::Contacts) => {
                contacts.push(ClassifiedFolder {
                    folder,
                    kind: class,
                });
            }
            (_, FolderClass::Skipped) => {}
        }
    }
    let well_known_roles = resolve_well_known_roles(client, url, mailbox_kind, logger)?;
    let root_folder_id = resolve_root_folder_id(client, url, mailbox_kind)?;
    Ok(FolderPlan {
        mail,
        calendar,
        contacts,
        well_known_roles,
        root_folder_id,
    })
}

fn resolve_well_known_roles(
    client: &EwsClient,
    url: &str,
    mailbox_kind: MailboxKind,
    logger: Logger,
) -> Result<HashMap<String, &'static str>, EwsError> {
    if !matches!(mailbox_kind, MailboxKind::Primary) {
        return Ok(HashMap::new());
    }
    let folders: Vec<FolderRef<'_>> = WELL_KNOWN_ROLES
        .iter()
        .map(|(d, _)| FolderRef::Distinguished(*d))
        .collect();
    let request = get_folder_body(&folders, FolderShape::IdOnly);
    let resp = match client.call(url, "GetFolder", &request) {
        Ok(r) => r,
        Err(e) => {
            if logger.enabled(LEVEL_PROGRESS) {
                eprintln!("GetFolder for distinguished ids failed: {e}; roles will be empty");
            }
            return Ok(HashMap::new());
        }
    };
    let messages =
        parse_response_messages(&resp.body, b"GetFolderResponseMessage").unwrap_or_default();
    let mut map: HashMap<String, &'static str> = HashMap::new();
    for (msg, (_, role)) in messages.into_iter().zip(WELL_KNOWN_ROLES.iter()) {
        if !msg.success {
            continue;
        }
        let Ok(Some(entry)) = parse_folder_inner(&msg.inner_xml) else {
            continue;
        };
        if entry.folder_id.id.is_empty() {
            continue;
        }
        if let Some(r) = role {
            map.insert(entry.folder_id.id, r);
        }
    }
    Ok(map)
}

fn resolve_root_folder_id(
    client: &EwsClient,
    url: &str,
    mailbox_kind: MailboxKind,
) -> Result<String, EwsError> {
    let root = mailbox_kind.distinguished_root();
    let body = get_folder_body(&[FolderRef::Distinguished(root)], FolderShape::IdOnly);
    let resp = client.call(url, "GetFolder", &body)?;
    let entries = parse_get_folder_response(&resp.body)?;
    Ok(entries
        .into_iter()
        .next()
        .map(|e| e.folder_id.id)
        .unwrap_or_default())
}

const WELL_KNOWN_ROLES: &[(DistinguishedFolderId, Option<&'static str>)] = &[
    (DistinguishedFolderId::Inbox, Some("inbox")),
    (DistinguishedFolderId::SentItems, Some("sent")),
    (DistinguishedFolderId::Drafts, Some("drafts")),
    (DistinguishedFolderId::DeletedItems, Some("trash")),
    (DistinguishedFolderId::JunkEmail, Some("junk")),
    (DistinguishedFolderId::Archive, Some("archive")),
    (DistinguishedFolderId::Outbox, None),
    (DistinguishedFolderId::ConversationHistory, None),
];

pub fn reconcile(
    conn: &mut Connection,
    source_id: i64,
    plan: &FolderPlan,
    mailbox_counts: &mut TypeCounts,
    calendar_counts: &mut TypeCounts,
    addressbook_counts: &mut TypeCounts,
    _logger: Logger,
) -> Result<(), Error> {
    let local_mailbox: HashMap<String, exchange_ews_ids::FolderRow> =
        exchange_ews_ids::folders_of_type(conn, source_id, exchange_ews_ids::MAILBOX)
            .map_err(|e| Error::Partial(e.to_string()))?;
    let local_calendar: HashMap<String, exchange_ews_ids::FolderRow> =
        exchange_ews_ids::folders_of_type(conn, source_id, exchange_ews_ids::CALENDAR)
            .map_err(|e| Error::Partial(e.to_string()))?;
    let local_addressbook: HashMap<String, exchange_ews_ids::FolderRow> =
        exchange_ews_ids::folders_of_type(conn, source_id, exchange_ews_ids::ADDRESS_BOOK)
            .map_err(|e| Error::Partial(e.to_string()))?;

    let mut id_to_local: HashMap<String, i64> = HashMap::new();
    for (id, row) in local_mailbox
        .iter()
        .chain(local_calendar.iter())
        .chain(local_addressbook.iter())
    {
        id_to_local.insert(id.clone(), row.local_id);
    }

    let ordered_mail = order_by_parent(&plan.mail);
    let ordered_calendar = order_by_parent(&plan.calendar);
    let ordered_contacts = order_by_parent(&plan.contacts);

    let mut server_ids_mail: Vec<String> = Vec::new();
    let mut server_ids_calendar: Vec<String> = Vec::new();
    let mut server_ids_addressbook: Vec<String> = Vec::new();

    for folder in &ordered_mail {
        server_ids_mail.push(folder.folder.folder_id.id.clone());
        let role = plan
            .well_known_roles
            .get(&folder.folder.folder_id.id)
            .copied();
        let mut ctx = UpsertCtx {
            conn,
            source_id,
            id_to_local: &mut id_to_local,
            local: &local_mailbox,
            counts: mailbox_counts,
            root_folder_id: &plan.root_folder_id,
        };
        upsert_mailbox(&mut ctx, folder, role)?;
    }
    for folder in &ordered_calendar {
        server_ids_calendar.push(folder.folder.folder_id.id.clone());
        let mut ctx = UpsertCtx {
            conn,
            source_id,
            id_to_local: &mut id_to_local,
            local: &local_calendar,
            counts: calendar_counts,
            root_folder_id: &plan.root_folder_id,
        };
        upsert_calendar(&mut ctx, folder)?;
    }
    for folder in &ordered_contacts {
        server_ids_addressbook.push(folder.folder.folder_id.id.clone());
        let mut ctx = UpsertCtx {
            conn,
            source_id,
            id_to_local: &mut id_to_local,
            local: &local_addressbook,
            counts: addressbook_counts,
            root_folder_id: &plan.root_folder_id,
        };
        upsert_address_book(&mut ctx, folder)?;
    }

    delete_vanished(
        conn,
        source_id,
        exchange_ews_ids::MAILBOX,
        "mailboxes",
        &local_mailbox,
        &server_ids_mail,
        mailbox_counts,
    )?;
    delete_vanished(
        conn,
        source_id,
        exchange_ews_ids::CALENDAR,
        "calendars",
        &local_calendar,
        &server_ids_calendar,
        calendar_counts,
    )?;
    delete_vanished(
        conn,
        source_id,
        exchange_ews_ids::ADDRESS_BOOK,
        "address_books",
        &local_addressbook,
        &server_ids_addressbook,
        addressbook_counts,
    )?;
    Ok(())
}

fn order_by_parent(folders: &[ClassifiedFolder]) -> Vec<&ClassifiedFolder> {
    let by_id: HashMap<&str, &ClassifiedFolder> = folders
        .iter()
        .map(|f| (f.folder.folder_id.id.as_str(), f))
        .collect();
    let mut depths: HashMap<&str, usize> = HashMap::new();
    fn depth_of<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a ClassifiedFolder>,
        memo: &mut HashMap<&'a str, usize>,
        seen: &mut std::collections::HashSet<&'a str>,
    ) -> usize {
        if let Some(d) = memo.get(id) {
            return *d;
        }
        if !seen.insert(id) {
            return 0;
        }
        let d = match by_id.get(id).and_then(|f| f.folder.parent_id.as_deref()) {
            Some(parent) if by_id.contains_key(parent) => 1 + depth_of(parent, by_id, memo, seen),
            _ => 0,
        };
        memo.insert(id, d);
        d
    }
    for f in folders {
        let id = f.folder.folder_id.id.as_str();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let d = depth_of(id, &by_id, &mut depths, &mut seen);
        depths.insert(id, d);
    }
    let mut out: Vec<&ClassifiedFolder> = folders.iter().collect();
    out.sort_by_key(|f| {
        depths
            .get(f.folder.folder_id.id.as_str())
            .copied()
            .unwrap_or(0)
    });
    out
}

struct UpsertCtx<'a> {
    conn: &'a mut Connection,
    source_id: i64,
    id_to_local: &'a mut HashMap<String, i64>,
    local: &'a HashMap<String, exchange_ews_ids::FolderRow>,
    counts: &'a mut TypeCounts,
    root_folder_id: &'a str,
}

fn upsert_mailbox(
    ctx: &mut UpsertCtx<'_>,
    folder: &ClassifiedFolder,
    role: Option<&'static str>,
) -> Result<(), Error> {
    let parent_local = parent_local_id(folder, ctx.id_to_local, ctx.root_folder_id);
    let tx = ctx
        .conn
        .unchecked_transaction()
        .map_err(|e| Error::Partial(e.to_string()))?;
    let existing = ctx.local.get(&folder.folder.folder_id.id);
    let local_id = if let Some(row) = existing {
        tx.execute(
            "UPDATE mailboxes SET name = ?1, parent_id = ?2, role = ?3 WHERE id = ?4",
            params![folder.folder.display_name, parent_local, role, row.local_id],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        exchange_ews_ids::update_change_key(
            &tx,
            ctx.source_id,
            exchange_ews_ids::MAILBOX,
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.fetched += 1;
        row.local_id
    } else {
        tx.execute(
            "INSERT INTO mailboxes (name, parent_id, role, sort_order, is_subscribed) \
             VALUES (?1, ?2, ?3, 0, 1)",
            params![folder.folder.display_name, parent_local, role],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        let new_id = tx.last_insert_rowid();
        exchange_ews_ids::insert(
            &tx,
            ctx.source_id,
            exchange_ews_ids::MAILBOX,
            folder.folder.parent_id.as_deref().unwrap_or(""),
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
            new_id,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.created += 1;
        new_id
    };
    tx.commit().map_err(|e| Error::Partial(e.to_string()))?;
    ctx.id_to_local
        .insert(folder.folder.folder_id.id.clone(), local_id);
    Ok(())
}

fn upsert_calendar(ctx: &mut UpsertCtx<'_>, folder: &ClassifiedFolder) -> Result<(), Error> {
    let tx = ctx
        .conn
        .unchecked_transaction()
        .map_err(|e| Error::Partial(e.to_string()))?;
    let existing = ctx.local.get(&folder.folder.folder_id.id);
    let local_id = if let Some(row) = existing {
        tx.execute(
            "UPDATE calendars SET name = ?1 WHERE id = ?2",
            params![folder.folder.display_name, row.local_id],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        exchange_ews_ids::update_change_key(
            &tx,
            ctx.source_id,
            exchange_ews_ids::CALENDAR,
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.fetched += 1;
        row.local_id
    } else {
        tx.execute(
            "INSERT INTO calendars (name, sort_order, is_subscribed, is_visible) \
             VALUES (?1, 0, 1, 1)",
            params![folder.folder.display_name],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        let new_id = tx.last_insert_rowid();
        exchange_ews_ids::insert(
            &tx,
            ctx.source_id,
            exchange_ews_ids::CALENDAR,
            folder.folder.parent_id.as_deref().unwrap_or(""),
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
            new_id,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.created += 1;
        new_id
    };
    tx.commit().map_err(|e| Error::Partial(e.to_string()))?;
    ctx.id_to_local
        .insert(folder.folder.folder_id.id.clone(), local_id);
    Ok(())
}

fn upsert_address_book(ctx: &mut UpsertCtx<'_>, folder: &ClassifiedFolder) -> Result<(), Error> {
    let tx = ctx
        .conn
        .unchecked_transaction()
        .map_err(|e| Error::Partial(e.to_string()))?;
    let existing = ctx.local.get(&folder.folder.folder_id.id);
    let local_id = if let Some(row) = existing {
        tx.execute(
            "UPDATE address_books SET name = ?1 WHERE id = ?2",
            params![folder.folder.display_name, row.local_id],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        exchange_ews_ids::update_change_key(
            &tx,
            ctx.source_id,
            exchange_ews_ids::ADDRESS_BOOK,
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.fetched += 1;
        row.local_id
    } else {
        tx.execute(
            "INSERT INTO address_books (name, sort_order, is_subscribed) \
             VALUES (?1, 0, 1)",
            params![folder.folder.display_name],
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        let new_id = tx.last_insert_rowid();
        exchange_ews_ids::insert(
            &tx,
            ctx.source_id,
            exchange_ews_ids::ADDRESS_BOOK,
            folder.folder.parent_id.as_deref().unwrap_or(""),
            &folder.folder.folder_id.id,
            &folder.folder.folder_id.change_key,
            new_id,
        )
        .map_err(|e| Error::Partial(e.to_string()))?;
        ctx.counts.created += 1;
        new_id
    };
    tx.commit().map_err(|e| Error::Partial(e.to_string()))?;
    ctx.id_to_local
        .insert(folder.folder.folder_id.id.clone(), local_id);
    Ok(())
}

fn parent_local_id(
    folder: &ClassifiedFolder,
    id_to_local: &HashMap<String, i64>,
    root_folder_id: &str,
) -> Option<i64> {
    let parent_id = folder.folder.parent_id.as_deref()?;
    if parent_id == root_folder_id || parent_id.is_empty() {
        return None;
    }
    id_to_local.get(parent_id).copied()
}

fn delete_vanished(
    conn: &mut Connection,
    source_id: i64,
    type_name: &str,
    table: &str,
    local: &HashMap<String, exchange_ews_ids::FolderRow>,
    server_ids: &[String],
    counts: &mut TypeCounts,
) -> Result<(), Error> {
    let server_set: std::collections::HashSet<&str> =
        server_ids.iter().map(String::as_str).collect();
    let mut vanished: Vec<(&str, &exchange_ews_ids::FolderRow)> = local
        .iter()
        .filter(|(id, _)| !server_set.contains(id.as_str()))
        .map(|(id, row)| (id.as_str(), row))
        .collect();
    if type_name == exchange_ews_ids::MAILBOX {
        vanished.sort_by_key(|(_, row)| std::cmp::Reverse(folder_depth(row, local)));
    }
    for (item_id, row) in vanished {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Partial(e.to_string()))?;
        let result = tx.execute(
            &format!("DELETE FROM {table} WHERE id = ?1"),
            params![row.local_id],
        );
        match result {
            Ok(_) => {
                exchange_ews_ids::delete_item(&tx, source_id, type_name, item_id)
                    .map_err(|e| Error::Partial(e.to_string()))?;
                tx.commit().map_err(|e| Error::Partial(e.to_string()))?;
                counts.deleted += 1;
            }
            Err(_) => {
                let _ = tx.rollback();
                counts.failed += 1;
            }
        }
    }
    Ok(())
}

fn folder_depth(
    row: &exchange_ews_ids::FolderRow,
    local: &HashMap<String, exchange_ews_ids::FolderRow>,
) -> usize {
    let mut depth = 0;
    let mut cursor = row.folder_id.as_str();
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    while !cursor.is_empty() && visited.insert(cursor) {
        match local.get(cursor) {
            Some(parent) => {
                depth += 1;
                cursor = parent.folder_id.as_str();
            }
            None => break,
        }
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, parent_ews_id: &str, local_id: i64) -> exchange_ews_ids::FolderRow {
        exchange_ews_ids::FolderRow {
            item_id: id.to_owned(),
            folder_id: parent_ews_id.to_owned(),
            change_key: String::new(),
            local_id,
        }
    }

    #[test]
    fn folder_depth_walks_parent_chain() {
        let mut local: HashMap<String, exchange_ews_ids::FolderRow> = HashMap::new();
        local.insert("ROOT".to_owned(), row("ROOT", "", 1));
        local.insert("CHILD".to_owned(), row("CHILD", "ROOT", 2));
        local.insert("GRAND".to_owned(), row("GRAND", "CHILD", 3));
        assert_eq!(folder_depth(local.get("ROOT").unwrap(), &local), 0);
        assert_eq!(folder_depth(local.get("CHILD").unwrap(), &local), 1);
        assert_eq!(folder_depth(local.get("GRAND").unwrap(), &local), 2);
    }

    #[test]
    fn folder_depth_handles_unknown_parent() {
        let mut local: HashMap<String, exchange_ews_ids::FolderRow> = HashMap::new();
        local.insert("X".to_owned(), row("X", "ORPHAN", 1));
        assert_eq!(folder_depth(local.get("X").unwrap(), &local), 0);
    }

    #[test]
    fn folder_depth_stops_on_cycle() {
        let mut local: HashMap<String, exchange_ews_ids::FolderRow> = HashMap::new();
        local.insert("A".to_owned(), row("A", "B", 1));
        local.insert("B".to_owned(), row("B", "A", 2));
        let d = folder_depth(local.get("A").unwrap(), &local);
        assert!(d <= 2);
    }
}
