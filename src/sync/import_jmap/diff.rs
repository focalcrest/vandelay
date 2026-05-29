/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashSet;

use crate::jmap::wire::JmapId;

pub struct IdSetDiff {
    pub new: Vec<JmapId>,
    pub vanished: Vec<JmapId>,
    pub present: Vec<JmapId>,
}

pub fn diff(server_ids: &[JmapId], local_ids: &HashSet<String>) -> IdSetDiff {
    let server_set: HashSet<&str> = server_ids.iter().map(|i| i.0.as_str()).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut new = Vec::new();
    let mut present = Vec::new();
    for id in server_ids {
        if !seen.insert(id.0.as_str()) {
            continue;
        }
        if local_ids.contains(&id.0) {
            present.push(id.clone());
        } else {
            new.push(id.clone());
        }
    }
    let mut vanished = Vec::new();
    for local in local_ids {
        if !server_set.contains(local.as_str()) {
            vanished.push(JmapId(local.clone()));
        }
    }
    vanished.sort_by(|a, b| a.0.cmp(&b.0));
    IdSetDiff {
        new,
        vanished,
        present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<JmapId> {
        v.iter().map(|s| JmapId((*s).to_owned())).collect()
    }

    fn locals(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn partitions_new_vanished_present() {
        let d = diff(&ids(&["a", "b", "c"]), &locals(&["b", "c", "d"]));
        assert_eq!(d.new, ids(&["a"]));
        assert_eq!(d.present, ids(&["b", "c"]));
        assert_eq!(d.vanished, ids(&["d"]));
    }

    #[test]
    fn empty_local_means_all_new() {
        let d = diff(&ids(&["x", "y"]), &locals(&[]));
        assert_eq!(d.new, ids(&["x", "y"]));
        assert!(d.vanished.is_empty());
        assert!(d.present.is_empty());
    }

    #[test]
    fn empty_server_means_all_vanished() {
        let d = diff(&ids(&[]), &locals(&["p", "q"]));
        assert!(d.new.is_empty());
        assert_eq!(d.vanished, ids(&["p", "q"]));
    }

    #[test]
    fn duplicate_server_ids_are_collapsed() {
        let d = diff(&ids(&["a", "a", "b"]), &locals(&["a"]));
        assert_eq!(d.new, ids(&["b"]));
        assert_eq!(d.present, ids(&["a"]));
        assert!(d.vanished.is_empty());
    }
}
