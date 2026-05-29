/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::{HashMap, HashSet};

pub struct TreeOrder {
    pub order: Vec<usize>,
    pub orphans: Vec<usize>,
    pub cycle_roots: Vec<usize>,
}

pub fn topo_order(
    items: &[(String, Option<String>)],
    existing_parents: &HashSet<String>,
) -> TreeOrder {
    let index: HashMap<&str, usize> = items
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (id.as_str(), i))
        .collect();

    let mut order = Vec::with_capacity(items.len());
    let mut orphans = Vec::new();
    let mut cycle_roots = Vec::new();
    let mut placed: HashSet<usize> = HashSet::new();

    loop {
        let mut progressed = false;
        for (i, (_, parent)) in items.iter().enumerate() {
            if placed.contains(&i) {
                continue;
            }
            let ready = match parent {
                None => true,
                Some(p) => match index.get(p.as_str()) {
                    Some(&pi) => placed.contains(&pi),
                    None => {
                        if !existing_parents.contains(p) {
                            orphans.push(i);
                        }
                        true
                    }
                },
            };
            if ready {
                order.push(i);
                placed.insert(i);
                progressed = true;
            }
        }
        if placed.len() == items.len() {
            break;
        }
        if !progressed {
            let mut remaining: Vec<usize> =
                (0..items.len()).filter(|i| !placed.contains(i)).collect();
            remaining.sort_by(|&a, &b| items[a].0.cmp(&items[b].0));
            let root = remaining[0];
            cycle_roots.push(root);
            order.push(root);
            placed.insert(root);
        }
    }

    TreeOrder {
        order,
        orphans,
        cycle_roots,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, parent: Option<&str>) -> (String, Option<String>) {
        (id.to_owned(), parent.map(str::to_owned))
    }

    fn empty() -> HashSet<String> {
        HashSet::new()
    }

    fn pos(order: &[usize], items: &[(String, Option<String>)], id: &str) -> usize {
        order
            .iter()
            .position(|&i| items[i].0 == id)
            .expect("id in order")
    }

    #[test]
    fn parents_precede_children() {
        let items = vec![item("c", Some("b")), item("b", Some("a")), item("a", None)];
        let t = topo_order(&items, &empty());
        assert_eq!(t.order.len(), 3);
        assert!(pos(&t.order, &items, "a") < pos(&t.order, &items, "b"));
        assert!(pos(&t.order, &items, "b") < pos(&t.order, &items, "c"));
        assert!(t.orphans.is_empty());
        assert!(t.cycle_roots.is_empty());
    }

    #[test]
    fn parent_not_fetched_and_not_existing_is_orphan() {
        let items = vec![item("x", Some("missing"))];
        let t = topo_order(&items, &empty());
        assert_eq!(t.order, vec![0]);
        assert_eq!(t.orphans, vec![0]);
    }

    #[test]
    fn parent_already_in_db_is_not_orphan() {
        let items = vec![item("x", Some("p"))];
        let existing: HashSet<String> = ["p".to_owned()].into_iter().collect();
        let t = topo_order(&items, &existing);
        assert_eq!(t.order, vec![0]);
        assert!(t.orphans.is_empty());
    }

    #[test]
    fn cycle_is_broken_by_smallest_id() {
        let items = vec![item("b", Some("a")), item("a", Some("b"))];
        let t = topo_order(&items, &empty());
        assert_eq!(t.order.len(), 2);
        assert_eq!(t.cycle_roots.len(), 1);
        assert_eq!(items[t.cycle_roots[0]].0, "a");
        assert!(pos(&t.order, &items, "a") < pos(&t.order, &items, "b"));
    }
}
