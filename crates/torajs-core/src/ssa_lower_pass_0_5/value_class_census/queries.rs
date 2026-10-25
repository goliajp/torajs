//! The census as the unit tests ask it: what it admits and why it keeps
//! each other candidate out.

use crate::ssa_lower_pass_0_5::arr_head_census;

/// The classes the value-class census admits, by name, sorted, each
/// field with what a new instance holds there.
pub fn value_class_inits(artifacts: &crate::check::CheckArtifacts) -> Vec<super::Admitted> {
    let (ast, types) = (&artifacts.5.mono_ast, &artifacts.1);
    let dead = arr_head_census::unreachable_twin_exprs(ast, types);
    let pinned = arr_head_census::program_pins_arr_head(ast, types, &dead);
    super::program_value_classes(ast, types, &dead, pinned)
}

/// Each candidate of the value-class census, by name, with the reason
/// it is kept out (`None`: admitted).
pub fn value_class_verdicts(
    artifacts: &crate::check::CheckArtifacts,
) -> Vec<(String, Option<String>)> {
    let (ast, types) = (&artifacts.5.mono_ast, &artifacts.1);
    let dead = arr_head_census::unreachable_twin_exprs(ast, types);
    let pinned = arr_head_census::program_pins_arr_head(ast, types, &dead);
    super::verdicts(ast, types, &dead, pinned)
        .into_iter()
        .map(|(c, v)| (c.to_string(), v.err()))
        .collect()
}

/// The classes the value-class census admits, by name, sorted.
pub fn value_class_names(artifacts: &crate::check::CheckArtifacts) -> Vec<String> {
    let held = std::collections::HashSet::new();
    let scans = crate::ssa_lower_ctx_state::ModuleScans::of(&artifacts.5.mono_ast, &artifacts.1);
    let p = crate::ssa_lower_pass_0_5::run(
        &artifacts.1,
        &mut crate::ssa::Module::default(),
        &artifacts.5,
        (&held, &scans),
    );
    let mut out: Vec<String> = (p.aliases.iter())
        .filter(|(_, t)| matches!(t, crate::ssa::Type::Obj(s) if p.facts.value_classes.contains(s)))
        .map(|(n, _)| n.clone())
        .collect();
    out.sort();
    out
}
