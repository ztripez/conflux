//! Advisory fusion eligibility.
//!
//! Elementwise kernels that share a table and cadence iterate the same rows on
//! the same schedule, so they *could* be computed in one pass. This module only
//! identifies such candidates; it never fuses them (there is no fusion pass in
//! MVP6). It does not prove safety either: members may have read-after-write
//! dependencies (one reads a column another writes), so a real fusion pass would
//! still need to order or reject them. The candidate is a starting point for that
//! analysis, not a green light.
//!
//! Lowering guarantees each rule writes a distinct `(table, column)` (it rejects
//! duplicate writers), so members of a group always write different columns.

use std::collections::HashMap;

use conflux_kernel::KernelReport;

use crate::report::FusionGroup;

/// Finds advisory fusion candidates: groups of two or more accepted kernels on
/// the same table firing at the same cadence, in first-seen order.
pub(crate) fn fusion_groups(kernels: &KernelReport) -> Vec<FusionGroup> {
    let mut order: Vec<(String, u64)> = Vec::new();
    let mut members: HashMap<(String, u64), Vec<String>> = HashMap::new();

    for kernel in &kernels.accepted {
        let key = (kernel.table_name.clone(), kernel.cadence.period);
        if !members.contains_key(&key) {
            order.push(key.clone());
        }
        members.entry(key).or_default().push(kernel.name.clone());
    }

    order
        .into_iter()
        .filter_map(|key| {
            let rules = members.remove(&key).expect("key came from members");
            if rules.len() < 2 {
                return None;
            }
            Some(FusionGroup {
                table: key.0,
                cadence: key.1,
                rules,
                note: "elementwise kernels on the same table and cadence; could fuse into one \
                       pass. Advisory only — not applied, and member read-after-write ordering \
                       is not verified (no fusion pass in MVP6)"
                    .to_string(),
            })
        })
        .collect()
}
