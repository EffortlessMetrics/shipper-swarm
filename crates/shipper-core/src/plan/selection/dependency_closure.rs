use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{Context, Result};
use cargo_metadata::PackageId;

pub(super) fn include_dependency_closure(
    selected: &[String],
    name_to_id: &BTreeMap<String, PackageId>,
    deps_of: &BTreeMap<PackageId, BTreeSet<PackageId>>,
) -> Result<BTreeSet<PackageId>> {
    let mut queue = VecDeque::new();
    let mut included = BTreeSet::new();

    seed_selected_packages(selected, name_to_id, &mut queue, &mut included)?;
    drain_dependency_queue(deps_of, &mut queue, &mut included);

    Ok(included)
}

fn seed_selected_packages(
    selected: &[String],
    name_to_id: &BTreeMap<String, PackageId>,
    queue: &mut VecDeque<PackageId>,
    included: &mut BTreeSet<PackageId>,
) -> Result<()> {
    for name in selected {
        let id = name_to_id
            .get(name)
            .with_context(|| format!("selected package not found or not publishable: {name}"))?
            .clone();

        if included.insert(id.clone()) {
            queue.push_back(id);
        }
    }
    Ok(())
}

fn drain_dependency_queue(
    deps_of: &BTreeMap<PackageId, BTreeSet<PackageId>>,
    queue: &mut VecDeque<PackageId>,
    included: &mut BTreeSet<PackageId>,
) {
    // Include internal dependencies transitively.
    while let Some(id) = queue.pop_front() {
        if let Some(deps) = deps_of.get(&id) {
            for dep in deps {
                if included.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
    }
}
