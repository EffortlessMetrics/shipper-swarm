use std::collections::{BTreeMap, BTreeSet};

use cargo_metadata::PackageId;

pub(super) fn build_indegree_map(
    included: &BTreeSet<PackageId>,
    deps_of: &BTreeMap<PackageId, BTreeSet<PackageId>>,
) -> BTreeMap<PackageId, usize> {
    included
        .iter()
        .map(|id| {
            let count = deps_of
                .get(id)
                .into_iter()
                .flatten()
                .filter(|dep| included.contains(*dep))
                .count();
            (id.clone(), count)
        })
        .collect()
}

pub(super) fn package_name(
    pkg_map: &BTreeMap<PackageId, &cargo_metadata::Package>,
    id: &PackageId,
) -> String {
    pkg_map
        .get(id)
        .map(|pkg| pkg.name.to_string())
        .unwrap_or_else(|| String::from("unknown"))
}

pub(super) fn initial_ready_set(
    indegree: &BTreeMap<PackageId, usize>,
    pkg_map: &BTreeMap<PackageId, &cargo_metadata::Package>,
) -> BTreeSet<(String, PackageId)> {
    indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| (package_name(pkg_map, id), id.clone()))
        .collect()
}
