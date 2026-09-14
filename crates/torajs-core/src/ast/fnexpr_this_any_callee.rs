//! The any-CALLEE argument position — the fifth call-position face
//! arm of [`super::fnexpr_this_call_faces`] (rotation 649).
//!
//! `executor(function () {}, function () { … this … })` where
//! `executor` is a binding of type `any`: every call of an `any`
//! value rides the any lane, and every path that lane can take with
//! an argument — call it plainly, `.call` / `.apply` it, store it and
//! call it later — reads `FLAG_CLOSURE_RECV_FIRST` before it puts
//! anything in argv[0] (the receiver-channel proof of
//! [`super::fnexpr_this_shapes`], kind 2). So a `this`-reading
//! function expression written straight into such an argument slot
//! is receiver-safe the moment it is promoted, and before this arm
//! the program was refused outright as an unclaimed receiver
//! position (test262 `Promise/reject/capability-invocation` spells
//! it).
//!
//! Which bindings count is decided the coarse way the rest of this
//! family decides it, with a refutation walk that only ever
//! over-refuses: a `let` / `const` / `var` annotated exactly `any`
//! (`collect_any_binding_names`, the same set the receiver arms
//! use), or a PARAMETER annotated `any` — provided every parameter
//! of that name anywhere in the program is annotated `any`, no
//! `let`-family binding of the name exists, and no function
//! declaration carries it. A same-named typed parameter elsewhere
//! would be a callee whose typed indirect call the promoted ABI
//! cannot survive, so one such spelling refuses the name program-wide.

use super::Stmt;
use super::desugar_with::walk::stmt_children_ref;
use std::collections::{HashMap, HashSet};

/// Parameter names that are `any` everywhere they are declared and
/// nowhere a `let`-family binding or a function name.
pub(super) fn any_param_callee_names(stmts: &[Stmt]) -> HashSet<String> {
    let mut all_any: HashMap<String, bool> = HashMap::new();
    let mut refused: HashSet<String> = HashSet::new();
    walk(stmts, &mut all_any, &mut refused);
    all_any
        .into_iter()
        .filter(|(n, ok)| *ok && !refused.contains(n))
        .map(|(n, _)| n)
        .collect()
}

fn walk(stmts: &[Stmt], all_any: &mut HashMap<String, bool>, refused: &mut HashSet<String>) {
    for s in stmts {
        match s {
            Stmt::FnDecl {
                name, params, body, ..
            } => {
                refused.insert(name.clone());
                for p in params {
                    let is_any = p.type_ann.as_deref() == Some("any") && !p.is_rest;
                    all_any
                        .entry(p.name.clone())
                        .and_modify(|ok| *ok &= is_any)
                        .or_insert(is_any);
                }
                walk(body, all_any, refused);
            }
            Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
                refused.insert(name.clone());
            }
            _ => {}
        }
        for c in stmt_children_ref(s) {
            walk(std::slice::from_ref(c), all_any, refused);
        }
    }
}
