/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::path::{Path, PathBuf};

use mockito::Matcher;
use serde_json::{Value, json};
use vandelay::db;
use vandelay::jmap::account::AccountSelector;
use vandelay::jmap::http::Auth;
use vandelay::logging::Logger;
use vandelay::sync::{self, CommonConfig, ConnectConfig, ExportConfig, ImportConfig};
use vandelay::types::ObjectType;

fn tmp() -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "vandelay-mocksync-{}-{:?}-{n}.sqlite",
        std::process::id(),
        std::thread::current().id(),
    ));
    let _ = std::fs::remove_file(&p);
    p
}

fn session_body(base: &str) -> String {
    json!({
        "apiUrl": format!("{base}/jmap/api"),
        "uploadUrl": format!("{base}/jmap/upload/{{accountId}}/"),
        "downloadUrl": format!("{base}/jmap/dl/{{accountId}}/{{blobId}}/{{type}}/{{name}}"),
        "capabilities": { "urn:ietf:params:jmap:core": {
            "maxObjectsInGet": 500, "maxObjectsInSet": 500, "maxCallsInRequest": 16,
            "maxConcurrentRequests": 4, "maxConcurrentUpload": 4,
            "maxSizeRequest": 10000000, "maxSizeUpload": 50000000
        } },
        "accounts": { "w": { "name": "alice",
            "accountCapabilities": { "urn:ietf:params:jmap:mail": {} } } }
    })
    .to_string()
}

#[test]
fn export_email_already_exists_is_matched_not_failed() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";

    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (1,'Inbox',NULL,'inbox')",
            [],
        )
        .unwrap();
        let blob = db::blobs::intern_blob(
            &conn,
            b"From: a@x\r\nSubject: hi\r\nMessage-ID: <m-1@h>\r\n\r\nbody",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO emails (blob_id,received_at,mailbox_ids,keywords)
             VALUES (?1,'2020-01-01T00:00:00Z','[1]','[\"$seen\"]')",
            rusqlite::params![blob],
        )
        .unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body(&base))
        .create();

    let _mq1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
            {"accountId":"w","ids":["t1"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _mq2 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
            {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _mg = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
            {"id":"t1","name":"Inbox","role":"inbox","parentId":null,
             "myRights":{"mayDelete":true}}],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
            {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _up = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .with_body(json!({"blobId":"UPLOADED"}).to_string())
        .expect(1)
        .create();
    let _imp = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/import".into()))
        .with_body(
            json!({"methodResponses":[["Email/import",{"accountId":"w",
                "notCreated":{"e1":{"type":"alreadyExists","existingId":"x9"}}},"i"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::export::run(
        CommonConfig {
            archive: archive.clone(),
            threads: 1,
            dry_run: false,
            max_retries: 1,
            allow_invalid_certs: false,
            logger: Logger::from_flags(true, 0),
        },
        ExportConfig {
            connect: ConnectConfig {
                url: base.clone(),
                auth: Auth::Basic {
                    user: "u".into(),
                    password: "p".into(),
                },
                account: AccountSelector::Id("w".into()),
            },
            objects: None,
            prune: false,
            yes: true,
        },
    )
    .expect("export run");

    let email = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "Email")
        .map(|(_, c)| c.clone())
        .expect("email counts");
    assert_eq!(email.created, 0);
    assert_eq!(email.failed, 0, "alreadyExists must not be a failure");
    assert_eq!(email.skipped, 1, "alreadyExists folds into matched");
    assert!(!summary.any_failed());

    let _ = std::fs::remove_file(&archive);
}

#[test]
fn email_export_sends_one_email_per_import_call() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";

    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (1,'Inbox',NULL,'inbox')",
            [],
        )
        .unwrap();
        for n in 1..=2 {
            let raw =
                format!("From: a@x\r\nSubject: m{n}\r\nMessage-ID: <m-{n}@h>\r\n\r\nbody {n}",);
            let blob = db::blobs::intern_blob(&conn, raw.as_bytes()).unwrap();
            conn.execute(
                "INSERT INTO emails (blob_id,received_at,mailbox_ids,keywords)
                 VALUES (?1,'2020-01-01T00:00:00Z','[1]','[]')",
                rusqlite::params![blob],
            )
            .unwrap();
        }
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body(&base))
        .create();

    let _mq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["t1"]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _mq_empty = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("Mailbox/query".into()),
            Matcher::Regex("anchor".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .create();
    let _mg = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"t1","name":"Inbox","role":"inbox","parentId":null,
                 "myRights":{"mayDelete":true}}],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _eq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();

    let _ups = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .with_body(json!({"blobId":"UP1"}).to_string())
        .expect(2)
        .create();

    let single_only = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("Email/import".into()),
            Matcher::Regex("e1".into()),
            Matcher::Regex("e2".into()),
        ]))
        .expect(0)
        .create();

    let imports = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/import".into()))
        .with_body(
            json!({"methodResponses":[["Email/import",
                {"accountId":"w","created":{"e":{"id":"x","blobId":"b","threadId":"t","size":10}}},"i"]]})
            .to_string(),
        )
        .expect(2)
        .create();

    let summary = sync::export::run(
        CommonConfig {
            archive: archive.clone(),
            threads: 1,
            dry_run: false,
            max_retries: 0,
            allow_invalid_certs: false,
            logger: Logger::from_flags(true, 0),
        },
        ExportConfig {
            connect: ConnectConfig {
                url: base.clone(),
                auth: Auth::Basic {
                    user: "u".into(),
                    password: "p".into(),
                },
                account: AccountSelector::Id("w".into()),
            },
            objects: None,
            prune: false,
            yes: true,
        },
    )
    .expect("export run");

    let email = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "Email")
        .map(|(_, c)| c.clone())
        .expect("email counts");
    assert_eq!(email.created, 2, "both emails imported in per-item rounds");
    assert_eq!(email.failed, 0, "no per-unit failure");
    assert!(!summary.any_failed(), "no whole-run failure");

    single_only.assert();
    imports.assert();
    let _ = std::fs::remove_file(&archive);
}

fn session_body_full(base: &str) -> String {
    json!({
        "apiUrl": format!("{base}/jmap/api"),
        "uploadUrl": format!("{base}/jmap/upload/{{accountId}}/"),
        "downloadUrl": format!("{base}/jmap/dl/{{accountId}}/{{blobId}}/{{type}}/{{name}}"),
        "capabilities": { "urn:ietf:params:jmap:core": {
            "maxObjectsInGet": 500, "maxObjectsInSet": 500, "maxCallsInRequest": 16,
            "maxConcurrentRequests": 4, "maxConcurrentUpload": 4,
            "maxSizeRequest": 10000000, "maxSizeUpload": 50000000
        } },
        "accounts": { "w": { "name": "alice",
            "accountCapabilities": {
                "urn:ietf:params:jmap:mail": {},
                "urn:ietf:params:jmap:sieve": {},
                "urn:ietf:params:jmap:contacts": {},
                "urn:ietf:params:jmap:calendars": {},
                "urn:ietf:params:jmap:filenode": {}
            } } }
    })
    .to_string()
}

fn import_cfg_objects(base: &str, objects: Vec<ObjectType>) -> ImportConfig {
    ImportConfig {
        connect: ConnectConfig {
            url: base.to_owned(),
            auth: Auth::Basic {
                user: "u".into(),
                password: "p".into(),
            },
            account: AccountSelector::Id("w".into()),
        },
        objects: Some(objects),
        allow_source_change: false,
    }
}

fn export_cfg_objects(base: &str, objects: Vec<ObjectType>) -> ExportConfig {
    ExportConfig {
        connect: ConnectConfig {
            url: base.to_owned(),
            auth: Auth::Basic {
                user: "u".into(),
                password: "p".into(),
            },
            account: AccountSelector::Id("w".into()),
        },
        objects: Some(objects),
        prune: false,
        yes: true,
    }
}

fn common(archive: &Path) -> CommonConfig {
    CommonConfig {
        archive: archive.to_path_buf(),
        threads: 1,
        dry_run: false,
        max_retries: 1,
        allow_invalid_certs: false,
        logger: Logger::from_flags(true, 0),
    }
}

fn common_dry(archive: &Path) -> CommonConfig {
    CommonConfig {
        dry_run: true,
        ..common(archive)
    }
}

fn anchor_terminator(server: &mut mockito::Server, api: &str, type_name: &str) -> mockito::Mock {
    server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex(format!("{type_name}/query")),
            Matcher::Regex("\"anchor\"".into()),
        ]))
        .with_body(
            json!({"methodResponses":[[format!("{type_name}/query"),
                {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect_at_most(1024)
        .create()
}

#[test]
fn import_removes_vanished_mailbox_from_archive_on_second_pass() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _term = anchor_terminator(&mut server, api, "Mailbox");

    let _q1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["A","B","C"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _g1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"A","name":"alpha","parentId":null,"role":null,"sortOrder":0,"isSubscribed":true},
                {"id":"B","name":"bravo","parentId":null,"role":null,"sortOrder":0,"isSubscribed":true},
                {"id":"C","name":"charlie","parentId":null,"role":null,"sortOrder":0,"isSubscribed":true}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let s1 = sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("first import");
    let mb1 = s1
        .per_type
        .iter()
        .find(|(t, _)| *t == "Mailbox")
        .map(|(_, c)| c.clone())
        .expect("mailbox counts");
    assert_eq!(mb1.fetched, 3, "first pass fetched all three mailboxes");
    {
        let conn = rusqlite::Connection::open(&archive).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM mailboxes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);
    }

    let _q2 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["A","C"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let s2 = sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("second import");
    let mb2 = s2
        .per_type
        .iter()
        .find(|(t, _)| *t == "Mailbox")
        .map(|(_, c)| c.clone())
        .expect("mailbox counts");
    assert_eq!(mb2.fetched, 0, "second pass fetches nothing");
    assert_eq!(mb2.deleted, 1, "vanished mailbox B is deleted");
    {
        let conn = rusqlite::Connection::open(&archive).unwrap();
        let names: Vec<String> = conn
            .prepare("SELECT name FROM mailboxes ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(names, vec!["alpha".to_owned(), "charlie".to_owned()]);
    }
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn import_present_item_change_on_server_is_not_propagated() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _term = anchor_terminator(&mut server, api, "Mailbox");

    let _q1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["A"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _g1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"A","name":"OriginalName","parentId":null,"role":null,"sortOrder":0,"isSubscribed":true}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("first import");

    let _q2 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["A"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let nope_get = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .expect(0)
        .create();

    let s2 = sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("second import");
    nope_get.assert();
    let mb2 = s2
        .per_type
        .iter()
        .find(|(t, _)| *t == "Mailbox")
        .map(|(_, c)| c.clone())
        .expect("mailbox counts");
    assert_eq!(
        mb2.fetched, 0,
        "present items must not be re-fetched: changes on server are intentionally ignored"
    );
    let name: String = rusqlite::Connection::open(&archive)
        .unwrap()
        .query_row("SELECT name FROM mailboxes WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "OriginalName", "archive name was not overwritten");
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn import_removes_vanished_email_and_drops_cross_ref() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _mbterm = anchor_terminator(&mut server, api, "Mailbox");
    let _emterm = anchor_terminator(&mut server, api, "Email");

    let _mbq1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["MX"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _mbg1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"MX","name":"Inbox","parentId":null,"role":"inbox","sortOrder":0,"isSubscribed":true}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eq1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":["E1","E2"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eg1 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/get".into()))
        .with_body(
            json!({"methodResponses":[["Email/get",{"accountId":"w","list":[
                {"id":"E1","blobId":"BLB1","receivedAt":"2020-01-01T00:00:00Z","mailboxIds":{"MX":true},"keywords":{"$seen":true}},
                {"id":"E2","blobId":"BLB2","receivedAt":"2020-01-02T00:00:00Z","mailboxIds":{"MX":true},"keywords":{}}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _dl1 = server
        .mock("GET", Matcher::Regex("/jmap/dl/w/BLB1/.*".into()))
        .with_body("From: a@x\r\nMessage-ID: <1@h>\r\n\r\nbody-one")
        .expect(1)
        .create();
    let _dl2 = server
        .mock("GET", Matcher::Regex("/jmap/dl/w/BLB2/.*".into()))
        .with_body("From: b@x\r\nMessage-ID: <2@h>\r\n\r\nbody-two")
        .expect(1)
        .create();

    sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox, ObjectType::Email]),
    )
    .expect("first import");
    {
        let conn = rusqlite::Connection::open(&archive).unwrap();
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM emails", [], |r| r.get(0))
                .unwrap(),
            2
        );
    }

    let _mbq2 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["MX"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eq2 = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":["E2"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let s2 = sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox, ObjectType::Email]),
    )
    .expect("second import");
    let em2 = s2
        .per_type
        .iter()
        .find(|(t, _)| *t == "Email")
        .map(|(_, c)| c.clone())
        .expect("email counts");
    assert_eq!(em2.deleted, 1, "vanished email is deleted from archive");
    let conn = rusqlite::Connection::open(&archive).unwrap();
    let remaining: i64 = conn
        .query_row("SELECT count(*) FROM emails", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1, "only the still-present email remains");
    let blobs: i64 = conn
        .query_row("SELECT count(*) FROM blobs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(blobs, 1, "blob GC reclaims orphan blob of deleted email");
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_missing_target_email_is_created_on_rerun() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (1,'Inbox',NULL,'inbox')",
            [],
        )
        .unwrap();
        for n in 1..=2 {
            let raw =
                format!("From: a@x\r\nSubject: m{n}\r\nMessage-ID: <m-{n}@h>\r\n\r\nbody {n}",);
            let blob = db::blobs::intern_blob(&conn, raw.as_bytes()).unwrap();
            let mm = vandelay::sync::keys::index_to_json(
                &vandelay::sync::emailmeta::email_index_from_blob(raw.as_bytes()),
            );
            conn.execute(
                "INSERT INTO emails (blob_id,received_at,mailbox_ids,keywords,message_match)
                 VALUES (?1,'2020-01-01T00:00:00Z','[1]','[]', ?2)",
                rusqlite::params![blob, mm],
            )
            .unwrap();
        }
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _mbterm = anchor_terminator(&mut server, api, "Mailbox");
    let _emterm = anchor_terminator(&mut server, api, "Email");

    let _mq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["T1"]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _mg = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"T1","name":"Inbox","role":"inbox","parentId":null,"myRights":{"mayDelete":true}}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();

    let _eq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":["X1"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eg = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/get".into()))
        .with_body(
            json!({"methodResponses":[["Email/get",{"accountId":"w","list":[
                {"id":"X1","messageId":["m-1@h"]}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let upload = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .with_body(json!({"blobId":"BUP"}).to_string())
        .expect(1)
        .create();
    let create = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/import".into()))
        .with_body(
            json!({"methodResponses":[["Email/import",{"accountId":"w",
                "created":{"e2":{"id":"Y2","blobId":"BUP","threadId":"t","size":10}}},"i"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::export::run(
        common(&archive),
        export_cfg_objects(&base, vec![ObjectType::Mailbox, ObjectType::Email]),
    )
    .expect("export");
    let email = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "Email")
        .map(|(_, c)| c.clone())
        .expect("email counts");
    assert_eq!(email.skipped, 1, "Message-ID match with X1 skips it");
    assert_eq!(email.created, 1, "missing email is created");
    assert_eq!(email.failed, 0);
    upload.assert();
    create.assert();
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_email_blake3_fallback_matches_when_no_message_id() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    let raw = "From: a@x\r\nSubject: hello\r\n\r\nno-msg-id-body";
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (1,'Inbox',NULL,'inbox')",
            [],
        )
        .unwrap();
        let blob = db::blobs::intern_blob(&conn, raw.as_bytes()).unwrap();
        let mm = vandelay::sync::keys::index_to_json(
            &vandelay::sync::emailmeta::email_index_from_blob(raw.as_bytes()),
        );
        conn.execute(
            "INSERT INTO emails (blob_id,received_at,mailbox_ids,keywords,message_match)
             VALUES (?1,'2020-01-01T00:00:00Z','[1]','[]', ?2)",
            rusqlite::params![blob, mm],
        )
        .unwrap();
    }

    let local_idx = vandelay::sync::emailmeta::email_index_from_blob(raw.as_bytes());
    assert!(local_idx.mids.is_empty(), "blob must lack Message-ID");

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _mbterm = anchor_terminator(&mut server, api, "Mailbox");
    let _emterm = anchor_terminator(&mut server, api, "Email");
    let _mq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["T1"]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _mg = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",{"accountId":"w","list":[
                {"id":"T1","name":"Inbox","role":"inbox","parentId":null,"myRights":{"mayDelete":true}}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _eq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":["X1"]},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eg_min = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("Email/get".into()),
            Matcher::Regex("messageId".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["Email/get",{"accountId":"w","list":[
                {"id":"X1","messageId":[]}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _eg_full = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("Email/get".into()),
            Matcher::Regex("from".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["Email/get",{"accountId":"w","list":[
                {"id":"X1","messageId":[],"from":[{"email":"a@x"}],"subject":"hello","sentAt":"","to":[]}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let no_upload = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .expect(0)
        .create();
    let no_import = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/import".into()))
        .expect(0)
        .create();

    let summary = sync::export::run(
        common(&archive),
        export_cfg_objects(&base, vec![ObjectType::Mailbox, ObjectType::Email]),
    )
    .expect("export");
    no_upload.assert();
    no_import.assert();
    let email = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "Email")
        .map(|(_, c)| c.clone())
        .expect("email counts");
    assert_eq!(email.skipped, 1, "BLAKE3 fallback matched target");
    assert_eq!(email.created, 0);
    assert_eq!(email.failed, 0);
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_address_book_creates_only_missing() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO address_books (id,name,description,is_default)
             VALUES (1,'Personal',NULL,1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO address_books (id,name,description,is_default)
             VALUES (2,'Work',NULL,0)",
            [],
        )
        .unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();

    let _g = server
        .mock("POST", api)
        .match_body(Matcher::Regex("AddressBook/get".into()))
        .with_body(
            json!({"methodResponses":[["AddressBook/get",{"accountId":"w","list":[
                {"id":"P","name":"personal","isDefault":true,"myRights":{"mayDelete":false}}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let create = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("AddressBook/set".into()),
            Matcher::Regex("Work".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["AddressBook/set",{"accountId":"w",
                "created":{"c2":{"id":"WID"}}},"s"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::export::run(
        common(&archive),
        export_cfg_objects(&base, vec![ObjectType::AddressBook]),
    )
    .expect("export");
    create.assert();
    let counts = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "AddressBook")
        .map(|(_, c)| c.clone())
        .expect("address book counts");
    assert_eq!(counts.skipped, 1, "Personal matches existing (case-fold)");
    assert_eq!(counts.created, 1, "Work is created");
    assert_eq!(counts.failed, 0);
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_calendar_creates_only_missing() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO calendars (id,name,is_default) VALUES (1,'Family',1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO calendars (id,name,is_default) VALUES (2,'Team',0)",
            [],
        )
        .unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _g = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Calendar/get".into()))
        .with_body(
            json!({"methodResponses":[["Calendar/get",{"accountId":"w","list":[
                {"id":"F","name":"family","isDefault":true,"myRights":{"mayDelete":false}}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let create = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("Calendar/set".into()),
            Matcher::Regex("Team".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["Calendar/set",{"accountId":"w",
                "created":{"c2":{"id":"TID"}}},"s"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::export::run(
        common(&archive),
        export_cfg_objects(&base, vec![ObjectType::Calendar]),
    )
    .expect("export");
    create.assert();
    let counts = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "Calendar")
        .map(|(_, c)| c.clone())
        .expect("calendar counts");
    assert_eq!(counts.skipped, 1, "Family matches existing (case-fold)");
    assert_eq!(counts.created, 1, "Team is created");
    assert_eq!(counts.failed, 0);
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_sieve_script_skips_matching_blob_content() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    let local_script = b"require [\"fileinto\"];\nkeep;\n";
    let new_script = b"require [\"reject\"];\nreject \"go away\";\n";
    {
        let conn = db::init::open(&archive).unwrap();
        let blob1 = db::blobs::intern_blob(&conn, local_script).unwrap();
        let blob2 = db::blobs::intern_blob(&conn, new_script).unwrap();
        conn.execute(
            "INSERT INTO sieve_scripts (id,name,is_active,blob_id) VALUES (1,'keepall',1,?1)",
            rusqlite::params![blob1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sieve_scripts (id,name,is_active,blob_id) VALUES (2,'reject',0,?1)",
            rusqlite::params![blob2],
        )
        .unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _g = server
        .mock("POST", api)
        .match_body(Matcher::Regex("SieveScript/get".into()))
        .with_body(
            json!({"methodResponses":[["SieveScript/get",{"accountId":"w","list":[
                {"id":"S1","name":"already-there","isActive":false,"blobId":"BSRV"}
            ],"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _dl = server
        .mock("GET", Matcher::Regex("/jmap/dl/w/BSRV/.*".into()))
        .with_body(local_script.as_slice())
        .expect(1)
        .create();
    let upload = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .with_body(json!({"blobId":"UPN"}).to_string())
        .expect(1)
        .create();
    let create = server
        .mock("POST", api)
        .match_body(Matcher::AllOf(vec![
            Matcher::Regex("SieveScript/set".into()),
            Matcher::Regex("reject".into()),
        ]))
        .with_body(
            json!({"methodResponses":[["SieveScript/set",{"accountId":"w",
                "created":{"c2":{"id":"S2"}}},"s"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _activate = server
        .mock("POST", api)
        .match_body(Matcher::Regex("onSuccessActivateScript".into()))
        .with_body(
            json!({"methodResponses":[["SieveScript/set",{"accountId":"w"},"a"]]}).to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::export::run(
        common(&archive),
        export_cfg_objects(&base, vec![ObjectType::SieveScript]),
    )
    .expect("export");
    upload.assert();
    create.assert();
    let counts = summary
        .per_type
        .iter()
        .find(|(t, _)| *t == "SieveScript")
        .map(|(_, c)| c.clone())
        .expect("sieve counts");
    assert_eq!(
        counts.skipped, 1,
        "matching-blob script is skipped regardless of name"
    );
    assert_eq!(counts.created, 1, "differing-blob script is created");
    assert_eq!(counts.failed, 0);
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn import_deeply_nested_mailbox_tree_orders_correctly() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";
    let archive = tmp();
    const DEPTH: usize = 10;

    let ids: Vec<String> = (0..DEPTH).map(|i| format!("L{i}")).collect();
    let mut servlist = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let parent = if i == 0 {
            Value::Null
        } else {
            Value::String(ids[i - 1].clone())
        };
        servlist.push(json!({
            "id": id, "name": format!("level{i}"),
            "parentId": parent, "role": null,
            "sortOrder": 0, "isSubscribed": true
        }));
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body_full(&base))
        .expect_at_least(1)
        .create();
    let _mbterm = anchor_terminator(&mut server, api, "Mailbox");
    let server_ids: Vec<Value> = ids.iter().rev().map(|s| Value::String(s.clone())).collect();
    let _q = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids": server_ids},"q"]]})
            .to_string(),
        )
        .expect(1)
        .create();
    let _g = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/get",
                {"accountId":"w","list": servlist,"notFound":[]},"g"]]})
            .to_string(),
        )
        .expect(1)
        .create();

    let summary = sync::import_jmap::run(
        common(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("import");
    assert!(!summary.any_failed(), "import had failures: {summary:?}");

    let conn = rusqlite::Connection::open(&archive).unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM mailboxes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n as usize, DEPTH);
    for i in 1..DEPTH {
        let pname: String = conn
            .query_row(
                "SELECT p.name FROM mailboxes c JOIN mailboxes p ON c.parent_id = p.id
                 WHERE c.name = ?1",
                rusqlite::params![format!("level{i}")],
                |r| r.get(0),
            )
            .unwrap_or_else(|e| panic!("level{i} parent lookup failed: {e}"));
        assert_eq!(pname, format!("level{}", i - 1));
    }
    let root: Option<i64> = conn
        .query_row(
            "SELECT parent_id FROM mailboxes WHERE name='level0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(root, None, "root has no parent");
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn export_dry_run_sends_no_mutating_calls() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";

    let archive = tmp();
    {
        let conn = db::init::open(&archive).unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (1,'Inbox',NULL,'inbox')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mailboxes (id,name,parent_id,role) VALUES (2,'Sent',NULL,NULL)",
            [],
        )
        .unwrap();
        let blob = db::blobs::intern_blob(
            &conn,
            b"From: a@x\r\nSubject: hi\r\nMessage-ID: <m-1@h>\r\n\r\nbody",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO emails (blob_id,received_at,mailbox_ids,keywords)
             VALUES (?1,'2020-01-01T00:00:00Z','[1]','[]')",
            rusqlite::params![blob],
        )
        .unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body(&base))
        .create();
    let _mq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();
    let _eq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Email/query".into()))
        .with_body(
            json!({"methodResponses":[["Email/query",
                {"accountId":"w","ids":[]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();

    let no_set = server
        .mock("POST", api)
        .match_body(Matcher::Regex(r"/(set|import)".into()))
        .expect(0)
        .create();
    let no_upload = server
        .mock("POST", Matcher::Regex("/jmap/upload/".into()))
        .expect(0)
        .create();

    let summary = sync::export::run(
        common_dry(&archive),
        export_cfg_objects(&base, vec![ObjectType::Mailbox, ObjectType::Email]),
    )
    .expect("dry-run export must succeed");
    assert!(
        !summary.any_failed(),
        "dry-run summary should not record failures: {summary:?}"
    );

    no_set.assert();
    no_upload.assert();
    let _ = std::fs::remove_file(&archive);
}

#[test]
fn import_dry_run_does_not_write_archive_or_download_blobs() {
    let mut server = mockito::Server::new();
    let base = server.url();
    let api = "/jmap/api";

    let archive = tmp();
    {
        let _ = db::init::open(&archive).unwrap();
    }

    let _root = server.mock("GET", "/").with_status(404).create();
    let _wk = server
        .mock("GET", "/.well-known/jmap")
        .with_body(session_body(&base))
        .create();
    let _term = anchor_terminator(&mut server, api, "Mailbox");
    let _mq = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/query".into()))
        .with_body(
            json!({"methodResponses":[["Mailbox/query",
                {"accountId":"w","ids":["s1"]},"q"]]})
            .to_string(),
        )
        .expect_at_least(1)
        .create();

    let no_set = server
        .mock("POST", api)
        .match_body(Matcher::Regex(r"/(set|import)".into()))
        .expect(0)
        .create();
    let no_get = server
        .mock("POST", api)
        .match_body(Matcher::Regex("Mailbox/get".into()))
        .expect(0)
        .create();
    let no_download = server
        .mock("GET", Matcher::Regex("/jmap/dl/".into()))
        .expect(0)
        .create();

    let summary = sync::import_jmap::run(
        common_dry(&archive),
        import_cfg_objects(&base, vec![ObjectType::Mailbox]),
    )
    .expect("dry-run import must succeed");
    assert!(
        !summary.any_failed(),
        "dry-run import should not record failures: {summary:?}"
    );

    no_set.assert();
    no_get.assert();
    no_download.assert();

    let conn = rusqlite::Connection::open(&archive).unwrap();
    let mailbox_rows: i64 = conn
        .query_row("SELECT count(*) FROM mailboxes", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mailbox_rows, 0, "dry-run must not insert into the archive");
    let source_rows: i64 = conn
        .query_row("SELECT count(*) FROM sources", [], |r| r.get(0))
        .unwrap();
    assert_eq!(source_rows, 0, "dry-run must not record the JMAP source");
    let _ = std::fs::remove_file(&archive);
}
