/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedMailbox {
    pub canonical_path: String,
    pub leaf: String,
    pub parent_path: Option<String>,
    pub role: Option<&'static str>,
    pub ephemeral: bool,
}

pub fn assemble<F>(input: &[String], role_for: F) -> Vec<PlannedMailbox>
where
    F: Fn(&str) -> Option<&'static str>,
{
    let explicit: BTreeSet<String> = input.iter().cloned().collect();
    let mut all_paths: BTreeSet<String> = BTreeSet::new();
    for p in &explicit {
        for prefix in prefixes(p) {
            all_paths.insert(prefix);
        }
    }

    let mut planned: Vec<PlannedMailbox> = Vec::with_capacity(all_paths.len());
    for path in all_paths {
        let (parent_path, leaf) = split_path(&path);
        let ephemeral = !explicit.contains(&path);
        let role = role_for(&path);
        planned.push(PlannedMailbox {
            canonical_path: path,
            leaf,
            parent_path,
            role,
            ephemeral,
        });
    }
    planned
}

pub fn vanished_depth_sort(paths: &mut [String]) {
    paths.sort_by(|a, b| depth_of(b).cmp(&depth_of(a)).then_with(|| a.cmp(b)));
}

fn prefixes(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').collect();
    let mut out = Vec::with_capacity(parts.len());
    for i in 1..=parts.len() {
        out.push(parts[..i].join("/"));
    }
    out
}

fn split_path(path: &str) -> (Option<String>, String) {
    match path.rsplit_once('/') {
        Some((parent, leaf)) => (Some(parent.to_owned()), leaf.to_owned()),
        None => (None, path.to_owned()),
    }
}

fn depth_of(path: &str) -> usize {
    path.bytes().filter(|b| *b == b'/').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_roles(_: &str) -> Option<&'static str> {
        None
    }

    fn inbox_role(p: &str) -> Option<&'static str> {
        if p == "Inbox" { Some("inbox") } else { None }
    }

    #[test]
    fn flat_paths_pass_through_unchanged() {
        let r = assemble(
            &["Inbox".to_owned(), "Sent".to_owned(), "Github".to_owned()],
            no_roles,
        );
        let names: Vec<&str> = r.iter().map(|m| m.canonical_path.as_str()).collect();
        assert_eq!(names, vec!["Github", "Inbox", "Sent"]);
        assert!(r.iter().all(|m| !m.ephemeral));
        assert!(r.iter().all(|m| m.parent_path.is_none()));
    }

    #[test]
    fn nested_path_creates_ephemeral_parent() {
        let r = assemble(
            &["Label_001/Label_002_under_Label_001".to_owned()],
            no_roles,
        );
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].canonical_path, "Label_001");
        assert!(r[0].ephemeral);
        assert_eq!(r[0].parent_path, None);
        assert_eq!(r[1].canonical_path, "Label_001/Label_002_under_Label_001");
        assert!(!r[1].ephemeral);
        assert_eq!(r[1].parent_path, Some("Label_001".to_owned()));
        assert_eq!(r[1].leaf, "Label_002_under_Label_001");
    }

    #[test]
    fn explicit_parent_is_not_marked_ephemeral() {
        let r = assemble(
            &[
                "Label_001".to_owned(),
                "Label_001/Sub".to_owned(),
                "Label_001/Other".to_owned(),
            ],
            no_roles,
        );
        let parent = r.iter().find(|m| m.canonical_path == "Label_001").unwrap();
        assert!(!parent.ephemeral);
    }

    #[test]
    fn three_level_deep_path_creates_all_missing_ancestors() {
        let r = assemble(&["A/B/C".to_owned()], no_roles);
        let names: Vec<&str> = r.iter().map(|m| m.canonical_path.as_str()).collect();
        assert_eq!(names, vec!["A", "A/B", "A/B/C"]);
        assert!(r[0].ephemeral);
        assert!(r[1].ephemeral);
        assert!(!r[2].ephemeral);
        assert_eq!(r[2].parent_path, Some("A/B".to_owned()));
    }

    #[test]
    fn role_closure_consulted_for_ephemeral_parents_too() {
        let r = assemble(&["Inbox/Sub".to_owned()], inbox_role);
        let inbox = r.iter().find(|m| m.canonical_path == "Inbox").unwrap();
        assert!(inbox.ephemeral, "auto-created from the nested label");
        assert_eq!(
            inbox.role,
            Some("inbox"),
            "an auto-created Inbox is still the inbox"
        );
    }

    #[test]
    fn role_closure_applies_to_explicit_top_level() {
        let r = assemble(&["Inbox".to_owned()], inbox_role);
        assert_eq!(r[0].role, Some("inbox"));
    }

    #[test]
    fn order_is_parents_before_children() {
        let r = assemble(
            &[
                "Zeta/Alpha".to_owned(),
                "Alpha".to_owned(),
                "Beta/Gamma/Delta".to_owned(),
            ],
            no_roles,
        );
        let names: Vec<&str> = r.iter().map(|m| m.canonical_path.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "Alpha",
                "Beta",
                "Beta/Gamma",
                "Beta/Gamma/Delta",
                "Zeta",
                "Zeta/Alpha",
            ]
        );
    }

    #[test]
    fn duplicate_inputs_dedup() {
        let r = assemble(
            &["Inbox".to_owned(), "Inbox".to_owned(), "Inbox".to_owned()],
            no_roles,
        );
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn vanished_depth_sort_orders_leaves_first() {
        let mut v = vec![
            "Foo".to_owned(),
            "Foo/Bar/Baz".to_owned(),
            "Other".to_owned(),
            "Foo/Bar".to_owned(),
        ];
        vanished_depth_sort(&mut v);
        assert_eq!(v, vec!["Foo/Bar/Baz", "Foo/Bar", "Foo", "Other",]);
    }

    #[test]
    fn vanished_depth_sort_handles_ties_lexicographically() {
        let mut v = vec!["B".to_owned(), "A".to_owned(), "C".to_owned()];
        vanished_depth_sort(&mut v);
        assert_eq!(v, vec!["A", "B", "C"]);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let r = assemble(&[], no_roles);
        assert!(r.is_empty());
    }
}
