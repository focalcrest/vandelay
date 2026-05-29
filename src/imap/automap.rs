/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub fn role_from_special_use(attributes: &[String]) -> Option<&'static str> {
    for attr in attributes {
        match attr.to_ascii_lowercase().as_str() {
            "\\all" => return Some("all"),
            "\\archive" => return Some("archive"),
            "\\drafts" => return Some("drafts"),
            "\\flagged" => return Some("flagged"),
            "\\important" => return Some("important"),
            "\\junk" => return Some("junk"),
            "\\sent" => return Some("sent"),
            "\\trash" => return Some("trash"),
            _ => {}
        }
    }
    None
}

pub fn role_for_folder(
    folder_name: &str,
    attributes: &[String],
    namespace_prefix: &str,
    automap_enabled: bool,
) -> Option<&'static str> {
    if folder_name.eq_ignore_ascii_case("INBOX") {
        return Some("inbox");
    }
    if let Some(r) = role_from_special_use(attributes) {
        return Some(r);
    }
    if !automap_enabled {
        return None;
    }
    let stripped = strip_prefix_case_insensitive(folder_name, namespace_prefix);
    let leaf = leaf_component(stripped);
    role_from_name(stripped, leaf)
}

fn role_from_name(full: &str, leaf: &str) -> Option<&'static str> {
    for (role, patterns) in ROLE_PATTERNS {
        for p in *patterns {
            if leaf.eq_ignore_ascii_case(p) || full.eq_ignore_ascii_case(p) {
                return Some(role);
            }
        }
    }
    None
}

fn strip_prefix_case_insensitive<'a>(name: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        return name;
    }
    if name.len() < prefix.len() {
        return name;
    }
    if name[..prefix.len()].eq_ignore_ascii_case(prefix) {
        &name[prefix.len()..]
    } else {
        name
    }
}

fn leaf_component(name: &str) -> &str {
    if let Some(idx) = name.rfind(['/', '.']) {
        &name[idx + 1..]
    } else {
        name
    }
}

const ROLE_PATTERNS: &[(&str, &[&str])] = &[
    (
        "sent",
        &[
            "Sent",
            "Sent Mail",
            "Sent Items",
            "Sent Messages",
            "Sent-Mail",
            "[Gmail]/Sent Mail",
            "[Gmail]/Sent",
        ],
    ),
    (
        "drafts",
        &["Drafts", "Draft", "Brouillons", "[Gmail]/Drafts"],
    ),
    (
        "trash",
        &[
            "Trash",
            "Deleted",
            "Deleted Items",
            "Deleted Messages",
            "[Gmail]/Trash",
            "[Gmail]/Bin",
        ],
    ),
    (
        "junk",
        &["Junk", "Spam", "Junk E-mail", "Junk Email", "[Gmail]/Spam"],
    ),
    ("all", &["[Gmail]/All Mail"]),
    ("archive", &["Archive", "Archives"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn inbox_always_maps_to_inbox() {
        assert_eq!(role_for_folder("INBOX", &[], "", true), Some("inbox"));
        assert_eq!(role_for_folder("inbox", &[], "", true), Some("inbox"));
        assert_eq!(role_for_folder("Inbox", &[], "", false), Some("inbox"));
    }

    #[test]
    fn special_use_drives_role() {
        let r = role_for_folder("Whatever", &attrs(&["\\Sent"]), "", false);
        assert_eq!(r, Some("sent"));
        let r = role_for_folder("Anywhere", &attrs(&["\\Drafts"]), "", true);
        assert_eq!(r, Some("drafts"));
    }

    #[test]
    fn special_use_table_covers_all_iana_roles() {
        for (attr, expected) in [
            ("\\All", "all"),
            ("\\Archive", "archive"),
            ("\\Drafts", "drafts"),
            ("\\Flagged", "flagged"),
            ("\\Important", "important"),
            ("\\Junk", "junk"),
            ("\\Sent", "sent"),
            ("\\Trash", "trash"),
        ] {
            let r = role_from_special_use(&attrs(&[attr]));
            assert_eq!(r, Some(expected), "for {attr}");
        }
    }

    #[test]
    fn special_use_attrs_unrelated_to_roles_yield_none() {
        for s in [
            "\\Subscribed",
            "\\HasChildren",
            "\\HasNoChildren",
            "\\Marked",
            "\\Unmarked",
            "\\Noselect",
            "\\Noinferiors",
            "\\Remote",
        ] {
            assert_eq!(role_from_special_use(&attrs(&[s])), None);
        }
    }

    #[test]
    fn name_heuristic_when_no_special_use() {
        assert_eq!(role_for_folder("Sent", &[], "", true), Some("sent"));
        assert_eq!(role_for_folder("Sent Items", &[], "", true), Some("sent"));
        assert_eq!(role_for_folder("Drafts", &[], "", true), Some("drafts"));
        assert_eq!(role_for_folder("Trash", &[], "", true), Some("trash"));
        assert_eq!(
            role_for_folder("Deleted Items", &[], "", true),
            Some("trash")
        );
        assert_eq!(role_for_folder("Junk", &[], "", true), Some("junk"));
        assert_eq!(role_for_folder("Spam", &[], "", true), Some("junk"));
        assert_eq!(role_for_folder("Archive", &[], "", true), Some("archive"));
        assert_eq!(
            role_for_folder("[Gmail]/All Mail", &[], "", true),
            Some("all")
        );
        assert_eq!(role_for_folder("Archives", &[], "", true), Some("archive"));
    }

    #[test]
    fn name_heuristic_disabled_when_automap_off() {
        assert_eq!(role_for_folder("Sent", &[], "", false), None);
        assert_eq!(role_for_folder("Trash", &[], "", false), None);
    }

    #[test]
    fn namespace_prefix_stripped_before_name_match() {
        assert_eq!(
            role_for_folder("INBOX.Sent", &[], "INBOX.", true),
            Some("sent")
        );
        assert_eq!(
            role_for_folder("INBOX.Drafts", &[], "INBOX.", true),
            Some("drafts")
        );
    }

    #[test]
    fn unknown_folder_yields_none() {
        assert_eq!(role_for_folder("My Custom Folder", &[], "", true), None);
        assert_eq!(role_for_folder("Projects/Alpha", &[], "", true), None);
    }

    #[test]
    fn case_insensitive_name_match() {
        assert_eq!(role_for_folder("sent", &[], "", true), Some("sent"));
        assert_eq!(role_for_folder("DRAFTS", &[], "", true), Some("drafts"));
    }

    #[test]
    fn all_special_use_wins_over_heuristic_archive() {
        let r = role_for_folder("[Gmail]/All Mail", &attrs(&["\\All"]), "", true);
        assert_eq!(r, Some("all"));
    }

    #[test]
    fn gmail_all_mail_no_attribute_is_all_not_archive() {
        let r = role_for_folder("[Gmail]/All Mail", &[], "", true);
        assert_eq!(r, Some("all"));
    }

    #[test]
    fn plain_archive_name_resolves_to_archive() {
        assert_eq!(role_for_folder("Archive", &[], "", true), Some("archive"));
        assert_eq!(role_for_folder("Archives", &[], "", true), Some("archive"));
    }
}
