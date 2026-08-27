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

use super::super::{Ast, Expr, ExprId, Param, Stmt};

mod defaults;
mod rebind;
use defaults::{DefaultFate, root_default_fates, toplevel_fn_names};
use rebind::{RowShape, rebuild};

/// 509-01 — widen and pad every row of a vtable slot until the rows
/// agree about what the slot takes. Answers the slot it could not,
/// which the caller refuses the program over.
pub fn join_vtable_slot_params(ast: &mut Ast) -> Option<String> {
    if ast.method_index.is_empty() {
        return None;
    }
    let sigs = collect_user_params(ast);
    let plan = match plan_slot_params(ast, &sigs) {
        Ok(plan) => plan,
        Err(msg) => return Some(msg),
    };
    if plan.is_empty() {
        return None;
    }
    apply_param_plan(ast, plan);
    None
}

/// One declaration's replacement user-parameter list, plus the
/// defaults that have to stop being supplied by the call site.
struct RowPlan {
    params: Vec<Param>,
    /// Parameters the slot's tail begins in front of — read back out
    /// of it at the head of the body (see [`rebind`]).
    rebound: Vec<String>,
    /// `(param index, param name, default expression)` in parameter
    /// order — one `if (p === undefined) p = <default>` guard each,
    /// spliced at the head of the row's body, and the parameter's own
    /// default replaced by the `undefined` literal.
    moved: Vec<(usize, String, ExprId)>,
}

/// The parameter list one slot settled on: an annotation per fixed
/// position, and the rest tail every row gains when some row declares
/// one.
#[derive(Debug, PartialEq)]
pub(super) struct SlotShape {
    pub fixed: Vec<Option<String>>,
    pub rest: Option<String>,
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
fn plan_slot_params(
    ast: &Ast,
    sigs: &HashMap<String, Vec<Param>>,
) -> Result<HashMap<String, RowPlan>, String> {
    let global_fns = toplevel_fn_names(ast);
    let mut plan: HashMap<String, RowPlan> = HashMap::new();
    for (m, by_root) in super::slot_groups_by_name(ast) {
        let stubs = super::dispatch_stub_names(sigs.keys(), &m);
        let mut shapes: Vec<SlotShape> = Vec::new();
        let mut roots: Vec<&String> = by_root.keys().collect();
        roots.sort();
        for root in roots {
            let rows = &by_root[root];
            // A row missing from `sigs` is not a `__this` shape at
            // all, so there is no list to align — leave the slot as
            // the ABI check found it.
            if rows.iter().any(|r| !sigs.contains_key(r)) {
                continue;
            }
            rest_starts_agree(rows, sigs, &m, root)?;
            if let Some(shape) = plan_one_root(ast, rows, sigs, &global_fns, &mut plan) {
                shapes.push(shape);
            }
        }
        // A dispatcher is minted only for a name whose owners form one
        // chain, so one shape is the case that matters; more than one
        // root means an unrelated class also declares the name and the
        // stub belongs to neither reading — leave it to the ABI check.
        if let ([shape], [_, ..]) = (shapes.as_slice(), stubs.as_slice()) {
            for d in &stubs {
                if let Some(RowShape { params, rebound }) = rebuild(&sigs[d], shape) {
                    plan.insert(
                        d.clone(),
                        RowPlan {
                            params,
                            rebound,
                            moved: Vec::new(),
                        },
                    );
                }
            }
        }
    }
    Ok(plan)
}

/// Two VARIADIC rows of one slot have to agree about where their rest
/// begins, and the ABI check cannot ask them.
///
/// It compares MACHINE shapes, and a rest array is one word like
/// anything else: `f(x, ...r)` beside `f(x, y, ...s)` fills the same
/// registers and passes it silently, then one of them reads
/// `f(1,2,3)` as `r = [2,3]` and the other as `y = 2, s = [3]`. No
/// single slot expresses both, so the refusal has to happen here,
/// where the parameter LISTS are still visible; declining silently is
/// what left the crash standing.
///
/// A row with no rest of its own is NOT in this disagreement, however
/// many fixed parameters it takes. There is exactly one reading of
/// where the tail begins — the variadic row's — and the other row
/// reads its extra parameters back out of that tail (see
/// [`rebind`]).
fn rest_starts_agree(
    rows: &[String],
    sigs: &HashMap<String, Vec<Param>>,
    m: &str,
    root: &str,
) -> Result<(), String> {
    let Some((a, na, b, nb)) = rest_mismatch(rows, sigs) else {
        return Ok(());
    };
    Err(format!(
        "ast: vtable slot for `{m}` of hierarchy `{root}` mixes a rest parameter with rows \
         that stop taking fixed parameters at different positions — `{a}` takes {na} before \
         its tail, `{b}` takes {nb}; one slot, one shape"
    ))
}

/// The two VARIADIC rows that disagree about where the tail begins —
/// the single reading of that disagreement. [`join_rows`] asks it
/// because it then has no shape to offer, and [`rest_starts_agree`]
/// asks it to say so; a slot the two answer differently about cannot
/// exist. Rows with no rest of their own are not consulted: they have
/// no opinion about where a tail starts, they only have parameters
/// that end up inside one.
fn rest_mismatch<'a>(
    rows: &'a [String],
    sigs: &HashMap<String, Vec<Param>>,
) -> Option<(&'a String, usize, &'a String, usize)> {
    let fixed_len = |r: &String| split_rest(&sigs[r]).0.len();
    let mut variadic = rows
        .iter()
        .filter(|r| split_rest(&sigs[r.as_str()]).1.is_some());
    let head = variadic.next()?;
    let odd = variadic.find(|r| fixed_len(r) != fixed_len(head))?;
    Some((head, fixed_len(head), odd, fixed_len(odd)))
}

/// Plan one root's rows, answering the shape the slot settled on —
/// or `None` when the slot is left exactly as the ABI check found it.
///
/// Two orderings are load-bearing. The widening question is asked
/// FIRST, against the rows' own annotations: a hierarchy whose rows
/// already agree keeps its narrow parameters, its narrow calls, and
/// its call-site-supplied defaults, so this pass costs it nothing.
/// Only once a slot is known to widen does the default question
/// arise — and then a defaulted position joins to `any` no matter
/// what the rows spell there, because the row that owns the default
/// is about to be entered with the pad's undefined and has to be able
/// to hold it.
fn plan_one_root(
    ast: &Ast,
    rows: &[String],
    sigs: &HashMap<String, Vec<Param>>,
    global_fns: &[String],
    plan: &mut HashMap<String, RowPlan>,
) -> Option<SlotShape> {
    let shape = join_rows(rows, sigs)?;
    if rows.iter().all(|r| rebuild(&sigs[r], &shape).is_none()) {
        return Some(shape);
    }
    // A default on a class method is supplied by the CALL SITE, not
    // by a guard in the body — `apply_default_args` pads by method
    // name, and only when every owner of that name agrees. Widening
    // this slot is exactly what makes them disagree, so the row that
    // owns the default would stop receiving it and answer NaN where
    // the language owes it 5. The default therefore moves into the
    // body, the way a plain function's already does; a default that
    // may not move keeps the slot refused (510-02).
    let fates = root_default_fates(ast, rows, sigs, global_fns)?;
    let mut shape = shape;
    for fate_row in &fates {
        for (i, f) in fate_row.iter().enumerate() {
            if matches!(f, DefaultFate::Move(_))
                && let Some(slot) = shape.fixed.get_mut(i)
            {
                *slot = Some("any".to_string());
            }
        }
    }
    for (r, fate_row) in rows.iter().zip(&fates) {
        let rebuilt = rebuild(&sigs[r], &shape);
        let changed = rebuilt.is_some();
        let RowShape { params, rebound } = rebuilt.unwrap_or_else(|| RowShape {
            params: sigs[r].clone(),
            rebound: Vec::new(),
        });
        let mut moved = Vec::new();
        for (i, f) in fate_row.iter().enumerate() {
            if let DefaultFate::Move(d) = f {
                // the name comes from the row's OWN list: a parameter
                // the tail swallowed is no longer in the new one
                moved.push((i, sigs[r][i].name.clone(), *d));
            }
        }
        if changed || !moved.is_empty() {
            plan.insert(
                r.clone(),
                RowPlan {
                    params,
                    rebound,
                    moved,
                },
            );
        }
    }
    Some(shape)
}

/// The shape one root's rows agree on: at each fixed position the
/// annotation every row spells there, or `any` when they differ or
/// when some row does not reach that far.
///
/// A rest parameter is the one thing a join cannot widen INTO: where
/// a row's fixed parameters end decides where its rest begins. The
/// slot's tail therefore begins where its VARIADIC rows say it does —
/// they have no other reading of the argument list — and every row
/// gains one, so a row that never declared a rest carries a
/// `__slotrest` it does not read, which is what makes the call site
/// pack for whichever row it resolved to. A row whose fixed
/// parameters run PAST that point reads the extra ones back out of the
/// tail at the head of its body (see [`rebind`]), and the tail widens
/// to `any[]` because it now carries them too.
///
/// Two variadic rows that disagree still have no shape — `None`, and
/// [`rest_starts_agree`] has already refused the program over the
/// same reading.
fn join_rows(rows: &[String], sigs: &HashMap<String, Vec<Param>>) -> Option<SlotShape> {
    let split = |r: &String| split_rest(&sigs[r]);
    let any_rest = rows.iter().any(|r| split(r).1.is_some());
    let fixed_len = |r: &String| split(r).0.len();
    rest_mismatch(rows, sigs).is_none().then_some(())?;
    // Where the tail begins is the VARIADIC rows' to say: that row has
    // no other reading of the argument list. A row with more fixed
    // parameters than that reads the extra ones back out of the tail
    // (see [`rebind`]); with no variadic row at all the slot is as wide
    // as its widest.
    let width = match rows
        .iter()
        .filter(|r| split(r).1.is_some())
        .map(fixed_len)
        .min()
    {
        Some(w) => w,
        None => rows.iter().map(fixed_len).max().unwrap_or(0),
    };
    let fixed = (0..width)
        .map(|i| {
            let head = split(&rows[0]).0.get(i).map(|p| p.type_ann.clone());
            match head {
                Some(a)
                    if rows.iter().all(|r| {
                        split(r).0.get(i).map(|p| p.type_ann.clone()) == Some(a.clone())
                    }) =>
                {
                    a
                }
                _ => Some("any".to_string()),
            }
        })
        .collect();
    let swallows = rows.iter().any(|r| fixed_len(r) > width);
    let rest = any_rest.then(|| {
        if swallows {
            // the tail now carries another row's fixed parameters too,
            // whatever those were spelled
            return "any[]".to_string();
        }
        let mut anns = rows
            .iter()
            .filter_map(|r| split(r).1)
            .map(|p| p.type_ann.clone().unwrap_or_else(|| "any[]".to_string()));
        let head = anns.next().unwrap_or_else(|| "any[]".to_string());
        if anns.all(|a| a == head) {
            head
        } else {
            "any[]".to_string()
        }
    });
    Some(SlotShape { fixed, rest })
}

/// One declaration's parameters split into its fixed ones and the
/// trailing rest it may declare.
pub(super) fn split_rest(ps: &[Param]) -> (&[Param], Option<&Param>) {
    match ps.last() {
        Some(l) if l.is_rest => (&ps[..ps.len() - 1], Some(l)),
        _ => (ps, None),
    }
}

/// Write each planned parameter list onto its declaration, behind the
/// receiver the plan was computed without, and splice the guards for
/// the defaults that left the call site.
///
/// The guards are built first — minting their expressions needs the
/// arena the walk borrows — and go in parameter order, so a later
/// default reading an earlier parameter sees the value that
/// parameter's own guard settled (§9.2 ordering).
fn apply_param_plan(ast: &mut Ast, mut plan: HashMap<String, RowPlan>) {
    let mut head: HashMap<String, Vec<Stmt>> = HashMap::new();
    for (name, row) in &mut plan {
        if row.moved.is_empty() && row.rebound.is_empty() {
            continue;
        }
        // The rebinds go FIRST: a guard for a swallowed parameter
        // assigns to the binding the rebind declares.
        let mut stmts = rebind::rebind_stmts(ast, &row.rebound);
        for (pi, p, d) in &row.moved {
            stmts.push(super::super::apply_args_materialize::build_default_guard(
                ast, p, *d,
            ));
            // A parameter that is STILL one keeps a default — now the
            // `undefined` literal, which needs no scope and is what
            // the guard fires on. That is also what evicts the method
            // NAME from `apply_default_args`' by-name table when some
            // unrelated class declares the same name with a real
            // default: an evicted name is padded by arity instead, and
            // arity sends the undefined this guard is waiting for.
            // Clearing the default outright let that other class's
            // literal be pasted into this row's call sites (`f(1)` on
            // a row owing 5 answered 4 with a stray 3 next door). A
            // parameter the tail swallowed has no slot to keep it in,
            // and needs none: a slot with a tail is already refused
            // that table, by the rule that a variadic row is padded an
            // EXTRA argument rather than an omitted one.
            if let Some(param) = row.params.get_mut(*pi)
                && &param.name == p
            {
                let undef = ast.add_expr(Expr::Ident("undefined".into()));
                param.default = Some(undef);
            }
        }
        head.insert(name.clone(), stmts);
    }
    let mut bodies = rebind::forward_bodies(ast, &plan);
    fn walk(
        stmts: &mut [Stmt],
        plan: &HashMap<String, RowPlan>,
        head: &mut HashMap<String, Vec<Stmt>>,
        bodies: &mut HashMap<String, Vec<Stmt>>,
    ) {
        for s in stmts {
            match s {
                Stmt::Multi(inner) => walk(inner, plan, head, bodies),
                Stmt::FnDecl {
                    name, params, body, ..
                } => {
                    if let Some(row) = plan.get(name.as_str()) {
                        params.truncate(1);
                        params.extend(row.params.iter().cloned());
                        if let Some(b) = bodies.remove(name.as_str()) {
                            *body = b;
                        }
                        if let Some(g) = head.remove(name.as_str()) {
                            super::super::apply_args_materialize::splice_guards(body, g);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    walk(&mut ast.stmts, &plan, &mut head, &mut bodies);
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

    fn names(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|r| (*r).to_string()).collect()
    }

    fn join(s: &HashMap<String, Vec<Param>>, rows: &[&str]) -> SlotShape {
        join_rows(&names(rows), s).expect("rows join")
    }

    /// Just the parameter list — most assertions do not care which
    /// names the tail swallowed.
    fn rb(have: &[Param], shape: &SlotShape) -> Option<Vec<Param>> {
        rebuild(have, shape).map(|r| r.params)
    }

    fn rest(name: &str, ann: Option<&str>) -> Param {
        Param {
            is_rest: true,
            ..p(name, ann)
        }
    }

    #[test]
    fn rows_that_agree_join_to_themselves() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("number"))]),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.fixed, vec![Some("number".to_string())]);
        // and nothing is rewritten
        assert!(rb(&s["a"], &shape).is_none());
    }

    #[test]
    fn unannotated_rows_stay_unannotated() {
        let s = sigs(&[("a", vec![p("x", None)]), ("b", vec![p("x", None)])]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.fixed, vec![None]);
        assert!(rb(&s["a"], &shape).is_none());
    }

    #[test]
    fn a_position_the_rows_spell_differently_widens() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("string"))]),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.fixed, vec![Some("any".to_string())]);
        let out = rb(&s["a"], &shape).expect("row widens");
        assert_eq!(out[0].name, "x");
        assert_eq!(out[0].type_ann.as_deref(), Some("any"));
    }

    #[test]
    fn a_position_a_row_never_declared_pads() {
        let s = sigs(&[
            ("a", vec![p("x", Some("number"))]),
            ("b", vec![p("x", Some("number")), p("y", Some("number"))]),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(
            shape.fixed,
            vec![Some("number".to_string()), Some("any".into())]
        );
        let narrow = rb(&s["a"], &shape).expect("narrow row pads");
        assert_eq!(narrow.len(), 2);
        assert_eq!(narrow[0].name, "x");
        assert_eq!(narrow[1].name, "__slotpad1");
        assert_eq!(narrow[1].type_ann.as_deref(), Some("any"));
        // the wide row keeps its own name at that position, widened
        let wide = rb(&s["b"], &shape).expect("wide row widens");
        assert_eq!(wide[1].name, "y");
        assert_eq!(wide[1].type_ann.as_deref(), Some("any"));
    }

    fn dflt(mut q: Param, ast: &mut Ast, e: Expr) -> Param {
        q.default = Some(ast.add_expr(e));
        q
    }

    /// The widening question comes first: rows that already agree
    /// keep their call-site-supplied defaults untouched.
    #[test]
    fn agreeing_rows_keep_their_call_site_defaults() {
        let mut ast = Ast::default();
        let y = dflt(p("y", Some("number")), &mut ast, Expr::Number(5.0));
        let s = sigs(&[
            ("a", vec![p("x", None), y.clone()]),
            ("b", vec![p("x", None), y]),
        ]);
        let mut plan = HashMap::new();
        let rows = vec!["a".to_string(), "b".to_string()];
        assert!(plan_one_root(&ast, &rows, &s, &[], &mut plan).is_some());
        assert!(plan.is_empty());
    }

    /// A widened slot moves the default into the row that owns it,
    /// and the position it sits at joins to `any` so the row can hold
    /// the pad's undefined.
    #[test]
    fn a_widened_slot_moves_its_default_into_the_body() {
        let mut ast = Ast::default();
        let y = dflt(p("y", Some("number")), &mut ast, Expr::Number(5.0));
        let s = sigs(&[("a", vec![p("x", None)]), ("b", vec![p("x", None), y])]);
        let mut plan = HashMap::new();
        let rows = vec!["a".to_string(), "b".to_string()];
        assert!(plan_one_root(&ast, &rows, &s, &[], &mut plan).is_some());
        let wide = &plan["b"];
        assert_eq!(wide.params[1].type_ann.as_deref(), Some("any"));
        assert_eq!(wide.moved.len(), 1);
        assert_eq!(wide.moved[0].0, 1);
        assert_eq!(wide.moved[0].1, "y");
        // the narrow row gains the pad and moves nothing
        assert_eq!(plan["a"].params[1].name, "__slotpad1");
        assert!(plan["a"].moved.is_empty());
    }

    /// A default the body-guard channel refuses keeps the whole slot
    /// refused — the ABI check then says so out loud.
    #[test]
    fn a_default_that_cannot_move_keeps_the_slot_refused() {
        let mut ast = Ast::default();
        let y = dflt(p("__yield_arg", None), &mut ast, Expr::Number(0.0));
        let s = sigs(&[("a", vec![p("x", None)]), ("b", vec![p("x", None), y])]);
        let mut plan = HashMap::new();
        let rows = vec!["a".to_string(), "b".to_string()];
        assert!(plan_one_root(&ast, &rows, &s, &[], &mut plan).is_none());
        assert!(plan.is_empty());
    }

    /// The `undefined` literal is already what a pad sends, so it is
    /// nothing to carry — the slot widens without moving anything.
    #[test]
    fn an_undefined_literal_default_is_not_moved() {
        let mut ast = Ast::default();
        let y = dflt(p("y", None), &mut ast, Expr::Ident("undefined".into()));
        let s = sigs(&[("a", vec![p("x", None)]), ("b", vec![p("x", None), y])]);
        let mut plan = HashMap::new();
        let rows = vec!["a".to_string(), "b".to_string()];
        assert!(plan_one_root(&ast, &rows, &s, &[], &mut plan).is_some());
        assert!(plan["b"].moved.is_empty());
        assert!(plan["b"].params[1].default.is_some());
    }

    /// Fixed arities agreeing, one row variadic: every row gains the
    /// tail, and the row that never declared one carries a
    /// `__slotrest` it does not read.
    #[test]
    fn a_rest_tail_reaches_every_row_of_the_slot() {
        let s = sigs(&[
            ("a", vec![p("x", None)]),
            ("b", vec![p("x", None), rest("r", Some("any[]"))]),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.rest.as_deref(), Some("any[]"));
        let narrow = rb(&s["a"], &shape).expect("narrow row gains the tail");
        assert_eq!(narrow.len(), 2);
        assert_eq!(narrow[1].name, "__slotrest");
        assert!(narrow[1].is_rest);
        // the variadic row keeps its own name and is left alone
        assert!(rb(&s["b"], &shape).is_none());
    }

    /// Fixed arities disagreeing with a rest in play unpacks the same
    /// argument list two ways, which one slot cannot express.
    #[test]
    fn rows_that_start_their_rest_at_different_positions_do_not_join() {
        let s = sigs(&[
            ("a", vec![p("x", None), rest("r", Some("any[]"))]),
            (
                "b",
                vec![p("x", None), p("y", None), rest("r2", Some("any[]"))],
            ),
        ]);
        assert!(join_rows(&names(&["a", "b"]), &s).is_none());
        assert!(rest_starts_agree(&names(&["a", "b"]), &s, "f", "A").is_err());
    }

    /// A defaulted scalar beside a variadic row: the tail begins
    /// where the VARIADIC row says it does, and the other row's `y`
    /// is inside it. Not a refusal — a rebinding.
    #[test]
    fn a_defaulted_row_beside_a_variadic_one_rebinds() {
        let mut ast = Ast::default();
        let y = dflt(p("y", Some("number")), &mut ast, Expr::Number(5.0));
        let s = sigs(&[
            ("a", vec![p("x", None), y]),
            ("b", vec![p("x", None), rest("r", Some("any[]"))]),
        ]);
        let rows = names(&["a", "b"]);
        assert!(rest_mismatch(&rows, &s).is_none());
        assert!(rest_starts_agree(&rows, &s, "f", "A").is_ok());
        let shape = join(&s, &["a", "b"]);
        // one fixed position, and a tail wide enough to hold what it
        // swallowed
        assert_eq!(shape.fixed.len(), 1);
        assert_eq!(shape.rest.as_deref(), Some("any[]"));
        let swallowed = rebuild(&s["a"], &shape).expect("the row rewrites");
        assert_eq!(swallowed.rebound, vec!["y".to_string()]);
        assert_eq!(swallowed.params.len(), 2);
        assert_eq!(swallowed.params[0].name, "x");
        assert!(swallowed.params[1].is_rest);
        // the variadic row is the one the tail's position came from,
        // so it is left exactly alone
        assert!(rebuild(&s["b"], &shape).is_none());
    }

    /// Two fixed positions past the tail's start both come back, in
    /// declaration order.
    #[test]
    fn every_swallowed_position_is_rebound() {
        let s = sigs(&[
            ("a", vec![p("x", None), p("y", None), p("z", None)]),
            ("b", vec![p("x", None), rest("r", Some("any[]"))]),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.fixed.len(), 1);
        let out = rebuild(&s["a"], &shape).expect("the row rewrites");
        assert_eq!(out.rebound, vec!["y".to_string(), "z".to_string()]);
    }

    /// A row that never reaches the tail's start still pads up to it.
    #[test]
    fn a_row_short_of_the_tail_pads_to_it() {
        let s = sigs(&[
            ("a", vec![p("x", None)]),
            (
                "b",
                vec![p("x", None), p("y", None), rest("r", Some("any[]"))],
            ),
        ]);
        let shape = join(&s, &["a", "b"]);
        assert_eq!(shape.fixed.len(), 2);
        let out = rebuild(&s["a"], &shape).expect("the row pads");
        assert!(out.rebound.is_empty());
        assert_eq!(out.params[1].name, "__slotpad1");
        assert!(out.params[2].is_rest);
    }

    /// Rows that all stop at the same position are not a mismatch,
    /// whether or not they declare a tail.
    #[test]
    fn agreeing_rest_starts_are_not_refused() {
        let s = sigs(&[
            ("a", vec![p("x", None)]),
            ("b", vec![p("x", None), rest("r", Some("any[]"))]),
        ]);
        let rows = names(&["a", "b"]);
        assert!(rest_mismatch(&rows, &s).is_none());
        assert!(rest_starts_agree(&rows, &s, "f", "A").is_ok());
    }

    /// Rest tails spelling different element types widen to `any[]`,
    /// the same rule the fixed positions take.
    #[test]
    fn rest_tails_that_disagree_widen() {
        let s = sigs(&[
            ("a", vec![p("x", None), rest("r", Some("number[]"))]),
            ("b", vec![p("x", None), rest("r", Some("string[]"))]),
        ]);
        assert_eq!(join(&s, &["a", "b"]).rest.as_deref(), Some("any[]"));
    }
}
