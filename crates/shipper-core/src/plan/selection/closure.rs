use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cargo_metadata::PackageId;

pub(super) fn close_over_dependencies(
    seed_ids: &BTreeSet<PackageId>,
    deps_of: &BTreeMap<PackageId, BTreeSet<PackageId>>,
) -> BTreeSet<PackageId> {
    let mut queue = VecDeque::from_iter(seed_ids.iter().cloned());
    let mut included = seed_ids.clone();

    while let Some(id) = queue.pop_front() {
        if let Some(deps) = deps_of.get(&id) {
            for dep in deps {
                if included.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
    }

    included
}
