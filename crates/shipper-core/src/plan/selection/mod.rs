use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use cargo_metadata::PackageId;

mod dependency_closure;
mod package_lookup;

use dependency_closure::include_dependency_closure;
use package_lookup::publishable_package_names;

pub(super) fn resolve_included_packages(
    selected_packages: Option<&[String]>,
    publishable: &BTreeSet<PackageId>,
    deps_of: &BTreeMap<PackageId, BTreeSet<PackageId>>,
    pkg_map: &BTreeMap<PackageId, &cargo_metadata::Package>,
) -> Result<BTreeSet<PackageId>> {
    let Some(selected) = selected_packages else {
        return Ok(publishable.clone());
    };

    let name_to_id = publishable_package_names(publishable, pkg_map)?;
    include_dependency_closure(selected, &name_to_id, deps_of)
}
