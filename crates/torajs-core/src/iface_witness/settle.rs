//! After the check: which interfaces keep their witness slots.
//!
//! An interface needs its slots only when a class instance the checker
//! let into it cannot be read at the interface's own field offsets —
//! the interface has a method, or the class lays its fields out
//! elsewhere or lacks one. Class instances reach an interface directly
//! or through a value of another interface it was let into, so the
//! notes propagate along those edges first. Every other interface
//! keeps plain field loads, and its thunks leave the AST before any
//! width analysis or lowering sees them: a program in which no class
//! instance stands behind an interface compiles as if there were no
//! witness rows at all.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::ast::{Ast, Stmt};

/// Keep the slots `ast` needs and drop the rest with their thunks.
/// `Err` names a class instance that would stand behind an interface
/// with slots but has no row there, or an interface value with slots
/// let into an object type that reads it at fixed offsets.
pub(crate) fn settle(ast: &mut Ast) -> Result<(), String> {
    let cand = super::admit::table();
    if cand.types.is_empty() {
        return Ok(());
    }
    let notes = super::admit::flows();
    let flows = class_flows(&notes);
    let slotted = slotted(&cand, &notes, &flows)?;
    drop_thunks(ast, &cand, &slotted)?;
    let mut kept = (*cand).clone();
    kept.faces.retain(|(i, _)| slotted.contains(i));
    kept.witnesses.retain(|i, _| slotted.contains(i));
    kept.types.retain(|i, _| slotted.contains(i));
    ast.iface_slots = kept;
    Ok(())
}

/// interface → every class whose instances reach it, directly or
/// through a value of another interface.
fn class_flows(notes: &super::admit::Flows) -> BTreeMap<String, BTreeSet<String>> {
    let mut flows: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (i, c) in &notes.classes {
        flows.entry(i.clone()).or_default().insert(c.clone());
    }
    loop {
        let mut grew = false;
        for (a, b) in &notes.edges {
            let (Some(b), Some(cs)) = (b, flows.get(a).cloned()) else {
                continue;
            };
            let into = flows.entry(b.clone()).or_default();
            for c in cs {
                grew |= into.insert(c);
            }
        }
        if !grew {
            return flows;
        }
    }
}

/// The interfaces that some class instance reaching them cannot be
/// read through at the interface's own offsets.
fn slotted(
    cand: &super::IfaceSlots,
    notes: &super::admit::Flows,
    flows: &BTreeMap<String, BTreeSet<String>>,
) -> Result<BTreeSet<String>, String> {
    let plain = |i: &str, c: &str| {
        !cand.with_methods.contains(i) && cand.plain.get(i).is_some_and(|cs| cs.contains(c))
    };
    let slotted: BTreeSet<String> = flows
        .iter()
        .filter(|(i, cs)| cs.iter().any(|c| !plain(i, c)))
        .map(|(i, _)| i.clone())
        .collect();
    for i in &slotted {
        let rows = &cand.witnesses[i];
        if let Some(c) = flows[i].iter().find(|c| !rows.contains(*c)) {
            return Err(format!(
                "not yet supported: an instance of class `{c}` read through interface `{i}`, which it does not implement with the interface's own member types"
            ));
        }
    }
    if let Some((a, _)) = notes
        .edges
        .iter()
        .find(|(a, b)| b.is_none() && slotted.contains(a))
    {
        return Err(format!(
            "not yet supported: a value of interface `{a}`, which class instances stand behind, read as an object type that is no interface"
        ));
    }
    Ok(slotted)
}

/// Take the thunks of every interface that keeps no slots out of the
/// AST. A later round (the mono pass settles again after it mints more
/// bodies) cannot bring dropped ones back.
fn drop_thunks(
    ast: &mut Ast,
    cand: &super::IfaceSlots,
    slotted: &BTreeSet<String>,
) -> Result<(), String> {
    let present: HashSet<&str> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut dropped: HashSet<String> = HashSet::new();
    for (i, names) in &cand.thunks {
        if !slotted.contains(i) {
            dropped.extend(names.iter().cloned());
        } else if names.iter().any(|n| !present.contains(n.as_str())) {
            return Err(format!(
                "not yet supported: interface `{i}` found to need its witness rows after they were dropped"
            ));
        }
    }
    ast.stmts
        .retain(|s| !matches!(s, Stmt::FnDecl { name, .. } if dropped.contains(name)));
    Ok(())
}
