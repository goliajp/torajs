//! 509-01 — one slot, one parameter list.
//!
//! The sibling question to [`super::join_vtable_slot_returns`], asked
//! at the other end of the signature. A call through a vtable slot is
//! emitted with a single body's signature, so two rows that take
//! differently-shaped parameter lists leave one of them entered under
//! the wrong ABI — and the check in
//! `ssa_lower_module_metadata_slot_abi` refuses the program rather
//! than let that happen silently.
//!
//! What it refuses, though, is ordinary JavaScript:
//!
//! ```text
//! class A { f(x: number) { return x } }
//! class B extends A { f(x: number, y: number) { return x + y } }
//! ```
//!
//! An override may take more parameters than the base declares; a
//! call that omits one binds `undefined` (§10.2.11), which is why bun
//! answers `NaN` for `B` and not an error. So the rows are not in
//! conflict about the language — only about the machine, exactly like
//! the return position. The slot's honest parameter list is the join
//! of its rows': as wide as the widest row, and `any` at every
//! position the rows spell differently, because `any` is the spelling
//! that holds a value and the undefined at once.
//!
//! The padding is what makes the join callable rather than merely
//! agreed: a row that gained `__slotpad<i>: any` is entered with the
//! ANY_UNDEF the call site's pad supplies (`pad_trailing_undef`, which
//! the slot lanes started honouring one commit before this one). That
//! ordering is load-bearing — with the pad missing, this pass turned
//! a loud refusal into a wrong answer, which is why its first version
//! was withdrawn.
//!
//! Rows that already agree are left untouched, so a hierarchy whose
//! overrides match keeps its narrow parameters and its narrow calls.

use std::collections::HashMap;

use super::super::{Ast, Param, Stmt};

/// 509-01 — widen and pad every row of a vtable slot until the rows
/// agree about what the slot takes.
pub fn join_vtable_slot_params(ast: &mut Ast) {
    if ast.method_index.is_empty() {
        return;
    }
    let sigs = collect_user_params(ast);
    let plan = plan_slot_params(ast, &sigs);
    if plan.is_empty() {
        return;
    }
    apply_param_plan(ast, &plan);
}

/// User-facing parameters of every top-level `__cm_` / `__dispatch_`
/// declaration — the receiver peeled off. Both spellings lead with a
/// bare `__this` by construction; anything else is not a slot row and
/// is left out, so the join never sees a list it cannot align.
fn collect_user_params(ast: &Ast) -> HashMap<String, Vec<Param>> {
    let mut out = HashMap::new();
    for stmt in super::super::toplevel_stmts_flat(ast) {
        if let Stmt::FnDecl { name, params, .. } = stmt
            && params.first().is_some_and(|p| p.name == "__this")
        {
            out.insert(name.clone(), params[1..].to_vec());
        }
    }
    out
}

/// The replacement user-parameter list of every declaration whose
/// slot needs one — rows first, then the `__dispatch_` stub that
/// shares their slot.
///
/// The population is [`super::slot_groups_by_name`], the same rows
/// `num_width::slot_abi` unions widths over and
/// `ssa_lower_module_metadata_slot_abi` checks shapes over: a slot
/// those three passes disagree about is a slot nobody checks.
fn plan_slot_params(ast: &Ast, sigs: &HashMap<String, Vec<Param>>) -> HashMap<String, Vec<Param>> {
    let mut plan: HashMap<String, Vec<Param>> = HashMap::new();
    for (m, by_root) in super::slot_groups_by_name(ast) {
        let stubs = super::dispatch_stub_names(sigs.keys(), &m);
        let mut shapes: Vec<Vec<Option<String>>> = Vec::new();
        for rows in by_root.values() {
            // A rest parameter has no fixed arity to join to, and a
            // row missing from `sigs` is not a `__this` shape at all.
            // Either way the honest move is to leave the slot as the
            // ABI check found it.
            if rows
                .iter()
                .any(|r| sigs.get(r).is_none_or(|ps| ps.iter().any(|p| p.is_rest)))
            {
                continue;
            }
            let shape = join_rows(rows, sigs);
            let rebuilt: Vec<(&String, Option<Vec<Param>>)> = rows
                .iter()
                .map(|r| (r, rebuild(&sigs[r], &shape)))
                .collect();
            // A default on a class method is supplied by the CALL
            // SITE, not by a guard in the body — `apply_default_args`
            // pads by method name, and only when every owner of that
            // name agrees. Widening this slot is exactly what makes
            // them disagree, so the row that owns the default would
            // stop receiving it and answer NaN where the language owes
            // it 5. Refusing the slot is the honest answer until a
            // class method's default lives in its body the way a plain
            // function's already does (510-02).
            if rebuilt.iter().any(|(_, ps)| ps.is_some())
                && rows
                    .iter()
                    .any(|r| sigs[r].iter().any(|p| p.default.is_some()))
            {
                continue;
            }
            for (r, ps) in rebuilt {
                if let Some(ps) = ps {
                    plan.insert(r.clone(), ps);
                }
            }
            shapes.push(shape);
        }
        // A dispatcher is minted only for a name whose owners form one
        // chain, so one shape is the case that matters; more than one
        // root means an unrelated class also declares the name and the
        // stub belongs to neither reading — leave it to the ABI check.
        if let ([shape], [_, ..]) = (shapes.as_slice(), stubs.as_slice()) {
            for d in &stubs {
                if let Some(ps) = rebuild(&sigs[d], shape) {
                    plan.insert(d.clone(), ps);
                }
            }
        }
    }
    plan
}

/// The joined annotation at each position of one root's rows: the
/// annotation every row spells there, or `any` when they differ or
/// when some row does not reach that far.
fn join_rows(rows: &[String], sigs: &HashMap<String, Vec<Param>>) -> Vec<Option<String>> {
    let width = rows.iter().map(|r| sigs[r].len()).max().unwrap_or(0);
    (0..width)
        .map(|i| {
            let head = sigs[&rows[0]].get(i).map(|p| p.type_ann.clone());
            match head {
                Some(a)
                    if rows
                        .iter()
                        .all(|r| sigs[r].get(i).map(|p| p.type_ann.clone()) == Some(a.clone())) =>
                {
                    a
                }
                _ => Some("any".to_string()),
            }
        })
        .collect()
}

/// One declaration's parameters rewritten to the joined shape, or
/// `None` when it already had it. Existing parameters keep their
/// names — the body reads them by name — and only the annotation
/// moves; the positions the row never declared arrive as pads.
fn rebuild(have: &[Param], shape: &[Option<String>]) -> Option<Vec<Param>> {
    let mut out: Vec<Param> = Vec::with_capacity(shape.len());
    let mut changed = have.len() != shape.len();
    for (i, want) in shape.iter().enumerate() {
        match have.get(i) {
            Some(p) => {
                changed |= p.type_ann != *want;
                out.push(Param {
                    type_ann: want.clone(),
                    ..p.clone()
                });
            }
            None => out.push(Param {
                name: format!("__slotpad{i}"),
                type_ann: Some("any".to_string()),
                default: None,
                is_rest: false,
            }),
        }
    }
    changed.then_some(out)
}

/// Write each planned parameter list onto its declaration, behind the
/// receiver the plan was computed without.
fn apply_param_plan(ast: &mut Ast, plan: &HashMap<String, Vec<Param>>) {
    fn walk(stmts: &mut [Stmt], plan: &HashMap<String, Vec<Param>>) {
        for s in stmts {
            match s {
                Stmt::Multi(inner) => walk(inner, plan),
                Stmt::FnDecl { name, params, .. } => {
                    if let Some(ps) = plan.get(name.as_str()) {
                        params.truncate(1);
                        params.extend(ps.iter().cloned());
                    }
                }
                _ => {}
            }
        }
    }
    walk(&mut ast.stmts, plan);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, ann: Option<&str>) -> Param {
        Param {
            name: name.into(),
            type_ann: ann.map(str::to_string),
            default: None,
            is_rest: false,
        }
    }

    fn sigs(rows: &[(&str, Vec<Param>)]) -> HashMap<String, Vec<Param>> {
        rows.iter()
            .map(|(n, ps)| ((*n).to_string(), ps.clone()))
            .collect()
    }

    #[test]
    fn rows_that_agree_join_to_themselves() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("number"))]),
        ]);
        let shape = join_rows(&["a".into(), "b".into()], &s);
        assert_eq!(shape, vec![Some("number".to_string())]);
        // and nothing is rewritten
        assert!(rebuild(&s["a"], &shape).is_none());
    }

    #[test]
    fn unannotated_rows_stay_unannotated() {
        let s = sigs(&[("a", vec![p("x", None)]), ("b", vec![p("x", None)])]);
        let shape = join_rows(&["a".into(), "b".into()], &s);
        assert_eq!(shape, vec![None]);
        assert!(rebuild(&s["a"], &shape).is_none());
    }

    #[test]
    fn a_position_the_rows_spell_differently_widens() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("string"))]),
        ]);
        let shape = join_rows(&["a".into(), "b".into()], &s);
        assert_eq!(shape, vec![Some("any".to_string())]);
        let out = rebuild(&s["a"], &shape).expect("row widens");
        assert_eq!(out[0].name, "x");
        assert_eq!(out[0].type_ann.as_deref(), Some("any"));
    }

    #[test]
    fn a_position_a_row_never_declared_pads() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("number")), p("y", Some("number"))]),
        ]);
        let shape = join_rows(&["a".into(), "b".into()], &s);
        assert_eq!(shape, vec![Some("number".to_string()), Some("any".into())]);
        let narrow = rebuild(&s["a"], &shape).expect("narrow row pads");
        assert_eq!(narrow.len(), 2);
        assert_eq!(narrow[0].name, "x");
        assert_eq!(narrow[1].name, "__slotpad1");
        assert_eq!(narrow[1].type_ann.as_deref(), Some("any"));
        // the wide row keeps its own name at that position, widened
        let wide = rebuild(&s["b"], &shape).expect("wide row widens");
        assert_eq!(wide[1].name, "y");
        assert_eq!(wide[1].type_ann.as_deref(), Some("any"));
    }
}
