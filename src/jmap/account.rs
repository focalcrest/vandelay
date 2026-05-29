/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::{Value, json};

use crate::error::Error;
use crate::jmap::http::HttpClient;
use crate::jmap::request::{Request, check_method_error};
use crate::jmap::session::Session;

#[derive(Debug, Clone)]
pub enum AccountSelector {
    Id(String),
    Name(String),
}

const OWNER_URN: &str = "urn:ietf:params:jmap:principals:owner";

pub fn resolve(
    selector: &AccountSelector,
    session: &Session,
    client: &HttpClient,
) -> Result<String, Error> {
    match selector {
        AccountSelector::Id(id) => Ok(id.clone()),
        AccountSelector::Name(name) => {
            for (id, account) in &session.accounts {
                if account.name == *name {
                    return Ok(id.clone());
                }
            }
            resolve_via_principal(name, session, client)
        }
    }
}

fn resolve_via_principal(
    name: &str,
    session: &Session,
    client: &HttpClient,
) -> Result<String, Error> {
    let mut req = Request::new();
    req.call(
        "Principal/query",
        json!({ "filter": { "name": name } }),
        "q",
    );
    req.call(
        "Principal/get",
        json!({
            "#ids": { "resultOf": "q", "name": "Principal/query", "path": "/ids" },
            "properties": ["id", "name", "accounts"]
        }),
        "g",
    );
    let resp = req
        .send(client, &session.api_url)
        .map_err(|e| Error::Account(format!("principal resolution request failed: {e}")))?;

    let query = resp
        .by_call_id("q")
        .map_err(|e| Error::Account(e.to_string()))?;
    check_method_error(query).map_err(|e| Error::Account(e.to_string()))?;
    let get = resp
        .by_call_id("g")
        .map_err(|e| Error::Account(e.to_string()))?;
    check_method_error(get).map_err(|e| Error::Account(e.to_string()))?;

    let list = get
        .args
        .get("list")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Account("Principal/get returned no list".to_owned()))?;

    let mut matches: Vec<&Value> = list
        .iter()
        .filter(|p| {
            p.get("name")
                .and_then(Value::as_str)
                .map(|n| n == name)
                .unwrap_or(false)
        })
        .collect();

    match matches.len() {
        0 => Err(Error::Account(format!(
            "account name '{name}' not found via Principal/query"
        ))),
        1 => extract_account_id(matches.remove(0), name),
        _ => {
            let ids: Vec<String> = matches
                .iter()
                .filter_map(|p| p.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect();
            Err(Error::Account(format!(
                "account name '{name}' is ambiguous; candidate principals: {}",
                ids.join(", ")
            )))
        }
    }
}

fn extract_account_id(principal: &Value, name: &str) -> Result<String, Error> {
    if let Some(accounts) = principal.get("accounts").and_then(Value::as_object) {
        for data in accounts.values() {
            if let Some(owner) = data.get(OWNER_URN)
                && let Some(id) = owner.get("accountIdForPrincipal").and_then(Value::as_str)
            {
                return Ok(id.to_owned());
            }
        }
        for data in accounts.values() {
            if let Some(cap) = data.as_object() {
                for v in cap.values() {
                    if let Some(id) = v.get("accountId").and_then(Value::as_str) {
                        return Ok(id.to_owned());
                    }
                }
            }
        }
    }
    principal
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Error::Account(format!(
                "principal for '{name}' has no resolvable data account id"
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with(name: &str, id: &str) -> Session {
        let raw = serde_json::json!({
            "apiUrl": "https://example.org/api",
            "uploadUrl": "https://example.org/upload",
            "downloadUrl": "https://example.org/download",
            "capabilities": {},
            "accounts": { id: { "name": name } }
        });
        serde_json::from_value(raw).unwrap()
    }

    fn client() -> HttpClient {
        HttpClient::new(
            crate::jmap::http::Auth::Bearer { token: "t".into() },
            crate::jmap::http::RetryPolicy::new(0),
            false,
        )
    }

    #[test]
    fn id_selector_is_verbatim() {
        let s = session_with("alice", "a");
        assert_eq!(
            resolve(&AccountSelector::Id("zzz".into()), &s, &client()).unwrap(),
            "zzz"
        );
    }

    #[test]
    fn name_selector_exact_case_sensitive_match() {
        let s = session_with("alice@example.org", "w");
        assert_eq!(
            resolve(
                &AccountSelector::Name("alice@example.org".into()),
                &s,
                &client()
            )
            .unwrap(),
            "w"
        );
    }

    #[test]
    fn substring_near_match_is_rejected_exactly() {
        let list = json!([
            { "id": "p1", "name": "alice2", "accounts": {} },
            { "id": "p2", "name": "alice", "accounts": {
                "w": { "urn:ietf:params:jmap:principals:owner":
                       { "accountIdForPrincipal": "w" } } } }
        ]);
        let matches: Vec<&Value> = list
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p.get("name").and_then(Value::as_str) == Some("alice"))
            .collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(extract_account_id(matches[0], "alice").unwrap(), "w");
    }

    #[test]
    fn extract_falls_back_to_principal_id() {
        let p = json!({ "id": "acc-7", "name": "bob", "accounts": {} });
        assert_eq!(extract_account_id(&p, "bob").unwrap(), "acc-7");
    }

    #[test]
    fn extract_uses_per_capability_account_id() {
        let p = json!({
            "id": "px",
            "name": "bob",
            "accounts": { "z": { "urn:ietf:params:jmap:mail": { "accountId": "data-9" } } }
        });
        assert_eq!(extract_account_id(&p, "bob").unwrap(), "data-9");
    }
}
