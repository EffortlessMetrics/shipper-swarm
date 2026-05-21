use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use cargo_metadata::PackageId;

pub(super) fn publishable_package_names(
    publishable: &BTreeSet<PackageId>,
    pkg_map: &BTreeMap<PackageId, &cargo_metadata::Package>,
) -> Result<BTreeMap<String, PackageId>> {
    let mut name_to_id = BTreeMap::new();
    for id in publishable {
        let pkg = pkg_map
            .get(id)
            .context("workspace package missing from metadata")?;
        name_to_id.insert(pkg.name.to_string(), id.clone());
    }
    Ok(name_to_id)
}
