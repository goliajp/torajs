//! A `.call` / `.apply` receiver spelled through an ALIAS.
//!
//! `namedfn_this_value` records the shape it must not touch: a
//! `Member` base, because "`t.call(r, 7)` answers the real receiver
//! today, and the forwarder pins `undefined`, so routing it here
//! would trade a correct answer for a wrong one". That protection is
//! written against the DECLARATION's own name. A second name for the
//! same function is not protected by it, and nothing else was
//! protecting it either:
//!
//! ```text
//! function foo() { return this }
//! const o = {}
//! foo.call(o) === o     tr true   bun true
//! const g = foo
//! g.call(o) === o       tr false  bun true
//! ```
//!
//! `const g = foo` is a value use, so the promotion rewrites it to
//! the `__forward_foo` cell — whose whole job is to pin the receiver
//! to `undefined` — and `g.call(o)` then rides the runtime lane with
//! the thisArg already thrown away. Silently: the call answers
//! `undefined` rather than refusing.
//!
//! The fix is to spell the receiver canonically BEFORE the promotion
//! runs. A `.call` / `.apply` / `.bind` receiver that is an alias for
//! a THIS-USING top-level FnDecl is rewritten to that declaration's
//! own name, so it reaches `desugar_function_prototype_methods` in
//! the shape that pass already handles correctly. Nothing else about
//! the alias moves: its other value uses still promote, so
//! `g === foo` keeps answering through the one forwarder cell.
//!
//! This-using is a real condition, not a scoping of convenience. A
//! this-free target's `.call` arm drops the thisArg outright — its
//! body cannot observe a receiver — and respelling the alias would
//! move that drop ahead of the argument's own evaluation, so
//! `f.call(withEffect(), 1, 2)` would stop printing `effect`
//! (measured: `fn-value-call-001` / `-apply-001`).
//!
//! Admission is narrow for a second reason: the arena walk is
//! scope-blind, so the name must be bound exactly once in the whole
//! statement tree — declaration, parameter, catch or loop variable
//! alike — and must never be assigned to. Anything else keeps
//! today's behavior.
//!
//! A STATIC method value is admitted on the same terms. `var r = C.m`
//! spells `Ident("__sm_C__m")` by the time this pass runs, and
//! `r.call(o)` through the escaped value's relay reached the mono
//! body — whose `this` is the class object by compile-time mint — so
//! `r.call({}) === C` answered true. The twin-bearing statics (the
//! ones whose bodies read `this`, `ast.smany_twins`) are respelled
//! exactly like the promoted declarations; the this-free ones keep
//! today's drop for the same evaluation-order reason.
//!
//! Pipeline: immediately before `promote_this_fn_values`, which is
//! the pass whose rewrite this one has to precede.

use super::{Ast, Expr, Stmt};
use std::collections::{HashMap, HashSet};

/// `__sm_C__m.bind(o)` → `__smany_C__m.bind(o)`.
///
/// The twin is where a `this`-reading static's receiver channel lives:
/// the mono has none at all (knife 3a), so its value is a shell that
/// pins the receiver to `undefined`, and the runtime `.bind` kernel
/// has nowhere to put the bound this. `C.m.call(o)` is devirtualized
/// to the twin by `desugar_function_prototype_methods`' knife 3c —
/// this is the same move for the one member that pass no longer
/// swallows, done here because by then the base is already a cell.
///
/// Twelve test262 cases in the async-private-static family are
/// `C.method.bind(C)`.
fn respell_static_bind_receivers(ast: &mut Ast) {
    if ast.smany_twins.is_empty() {
        return;
    }
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if name != "bind" {
            continue;
        }
        if let Expr::Ident(spelled) = ast.get_expr(*obj)
            && let Some(twin) = ast.smany_twins.get(spelled)
        {
            sites.push((i, twin.clone()));
        }
    }
    for (i, twin) in sites {
        let eid = ast.add_expr(Expr::Ident(twin));
        if let Expr::Member { obj, .. } = &mut ast.exprs[i] {
            *obj = eid;
        }
    }
}

pub fn canonicalize_fn_proto_receivers(ast: &mut Ast) {
    respell_static_bind_receivers(ast);
    // Only a THIS-USING declaration is in scope here. A this-free
    // target's `.call` arm drops the thisArg outright (its body
    // cannot observe a receiver), and respelling the alias would move
    // the drop earlier than the argument's own evaluation —
    // `f.call(withEffect(), 1, 2)` would lose the effect. The bug
    // being fixed is the thisArg going missing on the declarations
    // that DO read it.
    //
    // A `this`-reading STATIC method is the same question in a
    // second spelling: `C.m` already arrived as `Ident("__sm_C__m")`,
    // its mono has no receiver channel, and the twin-bearing statics
    // are exactly the ones whose bodies read `this` (knife 3a minted
    // `__smany_` for those alone). Respelled to the declaration's
    // name, `r.call(o)` reaches `desugar_function_prototype_methods`
    // in the shape its knife 3c devirtualizes to the twin — the same
    // route `C.m.call(o)` takes today.
    let fn_names: HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. }
                if ast.this_param_fns.contains(name) || ast.smany_twins.contains_key(name) =>
            {
                Some(name.clone())
            }
            _ => None,
        })
        .collect();
    if fn_names.is_empty() {
        return;
    }
    let aliases = collect_aliases(ast, &fn_names);
    if aliases.is_empty() {
        return;
    }
    let mut callees: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e {
            callees.insert(callee.0);
        }
    }
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Member { obj, name } = e else {
            continue;
        };
        if !callees.contains(&(i as u32)) || !matches!(name.as_str(), "call" | "apply" | "bind") {
            continue;
        }
        let Expr::Ident(spelled) = ast.get_expr(*obj) else {
            continue;
        };
        if let Some(target) = aliases.get(spelled) {
            sites.push((i, target.clone()));
        }
    }
    // A fresh Ident node per site, minted after the scan: the alias's
    // own binding expression is a different arena slot and stays put,
    // so its remaining value uses promote exactly as before.
    for (i, target) in sites {
        let eid = ast.add_expr(Expr::Ident(target));
        if let Expr::Member { obj, .. } = &mut ast.exprs[i] {
            *obj = eid;
        }
    }
}

/// Bindings that are nothing but a second name for a top-level
/// FnDecl, on the terms the module doc states.
fn collect_aliases(ast: &Ast, fn_names: &HashSet<String>) -> HashMap<String, String> {
    let mut aliases: HashMap<String, String> = HashMap::new();
    for s in super::toplevel_stmts_flat(ast) {
        let Stmt::LetDecl {
            name,
            init,
            type_ann: None,
            ..
        } = s
        else {
            continue;
        };
        if fn_names.contains(name) {
            continue;
        }
        if let Expr::Ident(target) = ast.get_expr(*init)
            && target != name
            && fn_names.contains(target)
        {
            aliases.insert(name.clone(), target.clone());
        }
    }
    if aliases.is_empty() {
        return aliases;
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    count_bindings(&ast.stmts, &mut counts);
    aliases.retain(|n, _| counts.get(n) == Some(&1));
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && let Expr::Ident(nm) = ast.get_expr(*target)
        {
            aliases.remove(nm);
        }
    }
    aliases
}

/// Count every name a statement tree BINDS — declarations,
/// parameters, catch and loop variables alike. Sibling of
/// `fnexpr_this_names::name_shadowed_elsewhere`, which answers the
/// same question for one name; this table has to answer it for many
/// at once.
fn count_bindings(stmts: &[Stmt], out: &mut HashMap<String, usize>) {
    fn note(n: &str, out: &mut HashMap<String, usize>) {
        *out.entry(n.to_string()).or_default() += 1;
    }
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. } => note(name, out),
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                note(name, out);
                for p in params {
                    note(&p.name, out);
                }
                count_bindings(body, out);
            }
            Stmt::ClassDecl { name, .. } => note(name, out),
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                if let Some(p) = catch_param {
                    note(p, out);
                }
                count_bindings(body, out);
                count_bindings(catch_body, out);
                if let Some(f) = finally_body {
                    count_bindings(f, out);
                }
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                note(var_name, out);
                count_bindings(std::slice::from_ref(body), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    count_bindings(std::slice::from_ref(i), out);
                }
                count_bindings(std::slice::from_ref(body), out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                count_bindings(std::slice::from_ref(body), out);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                count_bindings(std::slice::from_ref(then_branch), out);
                if let Some(e) = else_branch {
                    count_bindings(std::slice::from_ref(e), out);
                }
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    count_bindings(&c.body, out);
                }
                if let Some(d) = default {
                    count_bindings(d, out);
                }
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => count_bindings(inner, out),
            _ => {}
        }
    }
}
