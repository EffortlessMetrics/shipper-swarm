use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use cargo_metadata::PackageId;

use self::closure::close_over_dependencies;
use self::name_index::publishable_package_names;

mod closure;
mod name_index;

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
    let mut seeds = BTreeSet::new();

    for name in selected {
        let id = name_to_id
            .get(name)
            .with_context(|| format!("selected package not found or not publishable: {name}"))?
            .clone();
        seeds.insert(id);
    }

    Ok(close_over_dependencies(&seeds, deps_of))
}
