//! SuperProperty in OBJECT-LITERAL methods — §10.2.4 gives a method
//! shorthand a [[HomeObject]] (the literal itself), so `super.x`
//! inside it reads off GetPrototypeOf(home) (§13.3.7 GetSuperBase,
//! re-evaluated per access — `Object.setPrototypeOf(obj, …)` after
//! the definition changes what `super` sees, and the t262 family
//! asserts exactly that).
//!
//! The parser already encodes these sites off the `__superbase__` /
//! `__supercall__<m>` markers (primary_new_super; SuperProperty is
//! legal in a method body per §15.4.1). The class desugar consumes
//! the markers inside class bodies; until this pass, an object
//! literal's markers survived to the checker as unknown idents. This
//! pass runs right after `desugar_classes`, claims the shape it can
//! prove, and leaves everything else loud:
//!
//!   admitted — READS and CALLS in a declaration whose init IS the
//!   literal (`let/var/const obj = { m() { … super.x … } }`). The
//!   home binding pre-declares mutable (the declared name may be
//!   reassigned later; the HomeObject never moves), and each site
//!   becomes one `__torajs_super_prop_get` / `_call` off
//!   `Object.getPrototypeOf(__home_N)` — minted fresh per site, since
//!   GetSuperBase re-reads the prototype on every access.
//!
//!   The receiver those kernels carry is `__this`, the spelling
//!   `desugar_classes` pass 2 has already given every method-body
//!   `this` by the time this pass runs. That is what makes the call
//!   form claimable at all: it is the CALL SITE's receiver, so a
//!   method pulled off the literal and invoked against something else
//!   still reads its super base off the fixed home and its `this` off
//!   the call — the two things the home-object shortcut would have
//!   conflated.
//!
//!   Writes take the same route through `_set`: §9.1.9 OrdinarySet
//!   walks the BASE's chain to decide whether a setter runs and
//!   stores onto the RECEIVER otherwise, which is exactly the split
//!   the kernel's two object operands express.
//!
//!   left loud (marker → unknown ident, same posture as the class
//!   pass): `super.x++`; `super.m?.()`; a literal in any position
//!   other than a declaration init; and a method whose body nests
//!   ANOTHER literal-with-method — the inner method's markers belong
//!   to the inner home, and rewriting them against the outer one
//!   would be the silent wrong this table exists to prevent.
//!
use super::super_collect_prop::{
    SuperPropSite, SuperPropSites, collect_superprop_in_stmt, delete_operands,
};
use super::{Ast, Expr, ExprId, Stmt};

pub fn desugar_objlit_super(ast: &mut Ast) {
    let mut counter: u32 = 0;
    let mut stmts = std::mem::take(&mut ast.stmts);
    process_list(ast, &mut stmts, &mut counter);
    ast.stmts = stmts;
}

fn process_list(ast: &mut Ast, stmts: &mut [Stmt], counter: &mut u32) {
    for s in stmts.iter_mut() {
        process_stmt(ast, s, counter);
    }
}

fn process_stmt(ast: &mut Ast, s: &mut Stmt, counter: &mut u32) {
    if let Stmt::LetDecl { init, name, .. } = s
        && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
        && let Some(home) = claim_literal(ast, *init, counter)
    {
        // The methods capture `__home_N` BEFORE the literal finishes
        // evaluating (the closure mints inside the init), so the home
        // binding pre-declares mutable-undefined and is assigned back
        // right after the declaration — the capture is a box, so the
        // method bodies see the assignment (measured: the plain
        // `let y; const o = { m() { return y; } }; y = o;` spelling
        // answers `o`).
        let declared = name.clone();
        let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
        let home_let = Stmt::LetDecl {
            mutable: true,
            name: home.clone(),
            type_ann: Some("any".to_string()),
            init: undef,
            is_var: false,
        };
        let home_ref = ast.add_expr(Expr::Ident(home));
        let name_ref = ast.add_expr(Expr::Ident(declared));
        let assign = ast.add_expr(Expr::Assign {
            target: home_ref,
            value: name_ref,
        });
        let decl = std::mem::replace(s, Stmt::Multi(Vec::new()));
        *s = Stmt::Multi(vec![home_let, decl, Stmt::Expr(assign)]);
        return;
    }
    match s {
        Stmt::Block(list) | Stmt::Multi(list) => process_list(ast, list, counter),
        Stmt::FnDecl { body, .. } => process_list(ast, body, counter),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            process_stmt(ast, then_branch, counter);
            if let Some(eb) = else_branch {
                process_stmt(ast, eb, counter);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            process_stmt(ast, body, counter)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                process_stmt(ast, i, counter);
            }
            process_stmt(ast, body, counter);
        }
        Stmt::ForOf { body, .. } | Stmt::ForOfSplitIter { body, .. } => {
            process_stmt(ast, body, counter)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                process_list(ast, &mut c.body, counter);
            }
            if let Some(db) = default {
                process_list(ast, db, counter);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            process_list(ast, body, counter);
            process_list(ast, catch_body, counter);
            if let Some(fb) = finally_body {
                process_list(ast, fb, counter);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                process_stmt(ast, inner, counter);
            }
        }
        _ => {}
    }
}

/// Rewrite the literal's claimable super sites against a fresh home
/// binding. `Some(name)` = at least one site was rewritten and the
/// caller must hoist the literal into that binding; `None` = nothing
/// claimed (no marker, or the nested-literal bail), the declaration
/// stays as written.
fn claim_literal(ast: &mut Ast, objlit: ExprId, counter: &mut u32) -> Option<String> {
    let Expr::ObjectLit { fields } = ast.get_expr(objlit) else {
        return None;
    };
    let method_values: Vec<ExprId> = fields
        .iter()
        .map(|(_, v)| *v)
        .filter(|v| ast.objlit_method_exprs.contains(v))
        .collect();
    if method_values.is_empty() {
        return None;
    }
    let mut prop = SuperPropSites::default();
    let mut supercalls: Vec<ExprId> = Vec::new();
    let mut nested_home = false;
    for &mv in &method_values {
        let Expr::ArrowFn { body, .. } = ast.get_expr(mv) else {
            continue;
        };
        for st in body {
            collect_superprop_in_stmt(ast, st, &mut prop);
            scan_extra(ast, st, &mut supercalls, &mut nested_home);
        }
    }
    // A nested literal-with-method inside a method body: its markers
    // belong to ITS home. The read-only collector cannot tell them
    // apart, so the whole outer claim bails (loud beats wrong-home).
    if nested_home {
        return None;
    }
    let claimable = !supercalls.is_empty() || !prop.sites.is_empty();
    if !claimable {
        return None;
    }
    let home = format!("__home_{}", *counter);
    *counter += 1;
    // Sites are surveyed first and rewritten after: minting a node
    // invalidates the borrow the survey holds. Every claimed shape
    // becomes one kernel call carrying `this` — a literal method has
    // no static accessor walk at all (the class pass's `__cm_…_get`
    // shortcut has no counterpart here), so reading off the base
    // would run any getter against the base.
    let reads: Vec<(ExprId, KeyPlan)> = prop
        .sites
        .iter()
        .filter_map(|s| match s {
            SuperPropSite::Read { member, name } => Some((*member, KeyPlan::Name(name.clone()))),
            SuperPropSite::IndexRead { index_expr } => match ast.get_expr(*index_expr) {
                Expr::Index { index, .. } => Some((*index_expr, KeyPlan::Computed(*index))),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let deletes = delete_operands(ast);
    for (site, plan) in reads {
        let key = plan.mint(ast);
        if deletes.contains(&site) {
            // See `delete_operands` — a delete operand keeps the plain
            // member / index off the base.
            let base_expr = super_base_expr(ast, &home);
            let base = ast.add_expr(base_expr);
            // A `Member` name is an identifier string; a key that is
            // not a `&str` (lone surrogate) stays an Index read of the
            // literal, the same gate `parse_postfix` applies.
            ast.exprs[site.0 as usize] = match ast.get_expr(key) {
                Expr::String(n) if n.as_str().is_some() => Expr::Member {
                    obj: base,
                    name: n.as_str().unwrap().to_string(),
                },
                _ => Expr::Index {
                    obj: base,
                    index: key,
                },
            };
            continue;
        }
        let node = super_prop_kernel_call(ast, &home, key, Kernel::Get);
        ast.exprs[site.0 as usize] = node;
    }
    // Writes: the whole Assign node becomes the kernel call, so the
    // §9.1.9 receiver split (base decides, receiver stores) is the
    // kernel's business rather than something spelled in the AST.
    let writes: Vec<(ExprId, KeyPlan, ExprId)> = prop
        .sites
        .iter()
        .filter_map(|s| match s {
            SuperPropSite::AssignName {
                assign,
                name,
                value,
                ..
            } => Some((*assign, KeyPlan::Name(name.clone()), *value)),
            SuperPropSite::AssignIndex { target_index } => None.or_else(|| {
                let Expr::Index { index, .. } = ast.get_expr(*target_index) else {
                    return None;
                };
                let key = *index;
                let assign = assign_of_target(ast, *target_index)?;
                let Expr::Assign { value, .. } = ast.get_expr(assign) else {
                    return None;
                };
                Some((assign, KeyPlan::Computed(key), *value))
            }),
            _ => None,
        })
        .collect();
    for (assign, plan, value) in writes {
        let key = plan.mint(ast);
        let node = super_prop_kernel_call(ast, &home, key, Kernel::Set(value));
        ast.exprs[assign.0 as usize] = node;
    }
    let calls: Vec<(ExprId, KeyPlan)> = prop
        .sites
        .iter()
        .filter_map(|s| match s {
            SuperPropSite::CallIndex { call, index_expr } => match ast.get_expr(*index_expr) {
                Expr::Index { index, .. } => Some((*call, KeyPlan::Computed(*index))),
                _ => None,
            },
            _ => None,
        })
        .chain(supercalls.iter().filter_map(|&c| {
            let Expr::Call { callee, .. } = ast.get_expr(c) else {
                return None;
            };
            let Expr::Ident(n) = ast.get_expr(*callee) else {
                return None;
            };
            Some((c, KeyPlan::Name(n["__supercall__".len()..].to_string())))
        }))
        .collect();
    for (call, plan) in calls {
        let Expr::Call { args, .. } = ast.get_expr(call) else {
            continue;
        };
        let args = args.clone();
        let key = plan.mint(ast);
        let pack = ast.add_expr(Expr::Array(args));
        let node = super_prop_kernel_call(ast, &home, key, Kernel::Call(pack));
        ast.exprs[call.0 as usize] = node;
    }
    Some(home)
}

/// What a claimed site names: an already-minted key expression
/// (`super[k]`) or a name the parser folded into a marker.
enum KeyPlan {
    Computed(ExprId),
    Name(String),
}

impl KeyPlan {
    /// The survey runs while the arena is only readable; this mints
    /// the literal a name plan stands for once mutation is allowed.
    fn mint(self, ast: &mut Ast) -> ExprId {
        match self {
            KeyPlan::Computed(k) => k,
            KeyPlan::Name(n) => ast.add_expr(Expr::String(n.into())),
        }
    }
}

/// Which of the three SuperProperty kernels a site wants, and the
/// one extra operand two of them carry.
enum Kernel {
    Get,
    Set(ExprId),
    Call(ExprId),
}

/// `__torajs_super_prop_{get,set,call}(base, key, …, __this)`.
///
/// `__this`, not `Expr::This`: `desugar_classes` pass 2 has already
/// run and turned every method-body `this` into that spelling — a
/// bare `Expr::This` minted now reaches the checker unrewritten (it
/// says so by name).
fn super_prop_kernel_call(ast: &mut Ast, home: &str, key: ExprId, kind: Kernel) -> Expr {
    let base_expr = super_base_expr(ast, home);
    let base = ast.add_expr(base_expr);
    let recv = ast.add_expr(Expr::Ident("__this".to_string()));
    // The write puts its value BEFORE the receiver, matching the
    // kernel's `(base, key, value, this)` order; the call puts its
    // pack after.
    let (name, args) = match kind {
        Kernel::Get => ("__torajs_super_prop_get", vec![base, key, recv]),
        Kernel::Set(v) => ("__torajs_super_prop_set", vec![base, key, v, recv]),
        Kernel::Call(pack) => ("__torajs_super_prop_call", vec![base, key, recv, pack]),
    };
    let callee = ast.add_expr(Expr::Ident(name.to_string()));
    Expr::Call { callee, args }
}

/// The `Assign` whose target is `target` — `AssignIndex` records only
/// the target node, and the write rewrite replaces the whole
/// assignment.
fn assign_of_target(ast: &Ast, target: ExprId) -> Option<ExprId> {
    ast.exprs
        .iter()
        .position(|e| matches!(e, Expr::Assign { target: t, .. } if *t == target))
        .map(|i| ExprId(i as u32))
}

/// `Object.getPrototypeOf(__home_N)` — minted fresh per site so each
/// access re-reads the prototype (§13.3.7 GetSuperBase is not cached).
fn super_base_expr(ast: &mut Ast, home: &str) -> Expr {
    let obj = ast.add_expr(Expr::Ident("Object".to_string()));
    let gpo = ast.add_expr(Expr::Member {
        obj,
        name: "getPrototypeOf".to_string(),
    });
    let home_ref = ast.add_expr(Expr::Ident(home.to_string()));
    Expr::Call {
        callee: gpo,
        args: vec![home_ref],
    }
}

/// The two shapes `collect_superprop_in_stmt` does not surface: the
/// `__supercall__<m>` call marker (claimed here) and a nested
/// literal-with-method (the bail signal). Same walk skeleton as the
/// unclaimed-`this` gate: an expression arm the child list lacks
/// costs an under-claim, never a wrong rewrite.
fn scan_extra(ast: &Ast, s: &Stmt, supercalls: &mut Vec<ExprId>, nested_home: &mut bool) {
    for root in super::desugar_with::walk::stmt_exprs(s) {
        scan_extra_expr(ast, root, supercalls, nested_home);
    }
    for child in super::desugar_with::walk::stmt_children_ref(s) {
        scan_extra(ast, child, supercalls, nested_home);
    }
}

fn scan_extra_expr(ast: &Ast, eid: ExprId, supercalls: &mut Vec<ExprId>, nested_home: &mut bool) {
    match ast.get_expr(eid) {
        Expr::Call { callee, .. }
            if matches!(
                ast.get_expr(*callee),
                Expr::Ident(n) if n.starts_with("__supercall__")
            ) =>
        {
            supercalls.push(eid);
        }
        Expr::ObjectLit { fields }
            if fields
                .iter()
                .any(|(_, v)| ast.objlit_method_exprs.contains(v)) =>
        {
            *nested_home = true;
        }
        // An arrow inherits the enclosing method's home (§8.3.4) —
        // `collect_superprop_in_stmt` descends into arrow bodies, so
        // this walk must see the same sites. `expr_children`
        // deliberately stops at arrow bodies; descend by hand.
        Expr::ArrowFn { body, .. } => {
            for s in body {
                scan_extra(ast, s, supercalls, nested_home);
            }
            return;
        }
        _ => {}
    }
    for c in super::desugar_with::walk::expr_children(ast, eid) {
        scan_extra_expr(ast, c, supercalls, nested_home);
    }
}
