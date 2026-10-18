//! Whole-program census: which classes are value classes, whose
//! instances no code can tell apart from a copy of their fields, so an
//! array of them could hold the fields in place of pointers to cells.
//!
//! The class itself (`candidates`): a root nothing extends, not generic,
//! with no method, accessor, private or optional member, only `number`
//! and `boolean` fields, a factory that seeds every field with a literal
//! and hands the instance straight to the constructor, and a
//! constructor that only copies its parameters or literals into the
//! fields; no program spelling can freeze, delete, redefine or shadow a
//! field. Its fields must all lower to scalar slots, and no other name
//! may share its layout ([`value_sids`]).
//!
//! The program around it (`walk`): every expression the checker types
//! as holding an instance — the class, an array of it, or a container
//! class one of whose fields holds one — appears only where the shape
//! is known. An element read is the receiver of a field read or the
//! init of a `const` view that no call or array store can separate from
//! its reads (`view`); a construction is the argument of `push`; an
//! array is indexed, measured, pushed to, bound, stored into a slot of
//! its own type, or passed to and returned from a declared function
//! typed with it that nothing takes the address of. Anything else —
//! identity, an `any` slot, a builtin, a cast, a reflective road —
//! keeps the class out. The census is a whitelist over syntax with
//! checker types, like the other censuses of pass 0.5; a false negative
//! costs nothing but the pointer element the program has today.

mod decl;
mod program;
mod view;
mod walk;

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, ExprId, PropKey};
use crate::check::Type;
use crate::ssa;

use super::arr_head_census::ExprMask;
use program::Program;

/// One class that passed the declaration checks, with its fields in
/// declaration order.
pub(super) struct Candidate<'a> {
    name: &'a str,
    fields: Vec<&'a str>,
    /// User parameters of the constructor, in order (0 without one).
    arity: usize,
}

/// `TORAJS_VALUE_CLASS_STATS=1` — one line per class the census
/// admits, and per candidate it rules out with the first reason found.
fn stats() -> bool {
    std::env::var_os("TORAJS_VALUE_CLASS_STATS").is_some()
}

/// The names of the classes the census admits, sorted.
pub(crate) fn program_value_classes(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    dead: &ExprMask,
    arr_head_pinned: bool,
) -> Vec<String> {
    let cands = decl::candidates(ast);
    if cands.is_empty() {
        return Vec::new();
    }
    let stats = stats();
    let why = |class: &str, reason: &str| {
        if stats {
            eprintln!("[value_class] out: {class}: {reason}");
        }
    };
    let global = if !arr_head_pinned {
        Some("an array head may move")
    } else if ast.has_with_stmt || ast.program_reparents || ast.mints_absent_set {
        Some("a `with`, a reparent or an absent-set mint")
    } else if ast.accessor_field_names.may_freeze_or_delete() {
        Some("the program may freeze or delete")
    } else {
        None
    };
    if let Some(reason) = global {
        cands.iter().for_each(|c| why(c.name, reason));
        return Vec::new();
    }
    let prog = Program::index(ast, expr_types, dead);
    let mut out = Vec::new();
    for c in &cands {
        match walk::judge(&prog, c) {
            Ok(()) => out.push(c.name.to_string()),
            Err(reason) => why(c.name, &reason),
        }
    }
    out.sort();
    out
}

/// The sids of the admitted classes whose layout is all scalar slots
/// and is no other name's.
pub(crate) fn value_sids(
    names: &[String],
    aliases: &HashMap<String, ssa::Type>,
    struct_layouts: &[Vec<(PropKey, ssa::Type)>],
) -> HashSet<ssa::StructId> {
    use ssa::Type as T;
    let mut out = HashSet::new();
    let stats = !names.is_empty() && stats();
    for n in names {
        let Some(T::Obj(sid)) = aliases.get(n) else {
            continue;
        };
        let scalar = struct_layouts[sid.0 as usize]
            .iter()
            .all(|(_, t)| matches!(t, T::I64 | T::F64 | T::I32 | T::Bool));
        let shared = aliases.iter().any(|(m, t)| m != n && *t == T::Obj(*sid));
        if scalar && !shared {
            out.insert(*sid);
            if stats {
                eprintln!("[value_class] hit: {n}");
            }
        } else if stats {
            eprintln!(
                "[value_class] out: {n}: a field is not a scalar slot or another name shares the layout"
            );
        }
    }
    out
}
