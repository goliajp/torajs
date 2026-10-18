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

use std::collections::HashMap;

use crate::ast::{Ast, ExprId, PropKey};
use crate::check::Type;
use crate::num_width::WidthTable;
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
    /// The value each field of a new instance holds, by `fields`.
    inits: Vec<FieldInit>,
}

impl<'a> Candidate<'a> {
    /// A class kept out before it became a candidate.
    fn named(name: &'a str) -> Self {
        Candidate {
            name,
            fields: Vec::new(),
            arity: 0,
            inits: Vec::new(),
        }
    }
}

/// What a field of a newly built instance holds: an argument of the
/// construction (by position), or a literal the factory or the
/// constructor writes. An array holding the class in place builds an
/// element from these, in its slot, with no factory call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldInit {
    Arg(usize),
    Number(f64),
    Bool(bool),
}

/// An admitted class: its name, and each field with what a new
/// instance holds there.
pub type Admitted = (String, Vec<(String, FieldInit)>);

/// `TORAJS_VALUE_CLASS_STATS=1` — one line per class the census
/// admits, and per candidate it rules out with the first reason found.
fn stats() -> bool {
    std::env::var_os("TORAJS_VALUE_CLASS_STATS").is_some()
}

/// The census walks the whole program once per candidate; past this
/// many, every candidate is judged out rather than paid for.
const MAX_CANDIDATES: usize = 8;

/// The classes the census admits, sorted by name.
pub(crate) fn program_value_classes(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    dead: &ExprMask,
    arr_head_pinned: bool,
) -> Vec<Admitted> {
    let stats = stats();
    let mut out = Vec::new();
    for (c, verdict) in judged(ast, expr_types, dead, arr_head_pinned) {
        match verdict {
            Ok(()) => {
                let inits = (c.fields.iter().zip(&c.inits))
                    .map(|(f, i)| (f.to_string(), *i))
                    .collect();
                out.push((c.name.to_string(), inits));
            }
            Err(reason) if stats => eprintln!("[value_class] out: {}: {reason}", c.name),
            Err(_) => {}
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Each candidate class in declaration order, admitted or with the
/// first reason found to keep it out.
pub(crate) fn verdicts<'a>(
    ast: &'a Ast,
    expr_types: &HashMap<ExprId, Type>,
    dead: &ExprMask,
    arr_head_pinned: bool,
) -> Vec<(&'a str, Result<(), String>)> {
    (judged(ast, expr_types, dead, arr_head_pinned).into_iter())
        .map(|(c, v)| (c.name, v))
        .collect()
}

fn judged<'a>(
    ast: &'a Ast,
    expr_types: &HashMap<ExprId, Type>,
    dead: &ExprMask,
    arr_head_pinned: bool,
) -> Vec<(Candidate<'a>, Result<(), String>)> {
    let (cands, refused) = decl::candidates(ast);
    let refused = (refused.into_iter()).map(|(name, reason)| (Candidate::named(name), Err(reason)));
    let global = if cands.len() > MAX_CANDIDATES {
        Some(format!("more than {MAX_CANDIDATES} candidate classes"))
    } else if !arr_head_pinned {
        Some("an array head may move".into())
    } else if ast.has_with_stmt || ast.program_reparents || ast.mints_absent_set {
        Some("a `with`, a reparent or an absent-set mint".into())
    } else if ast.accessor_field_names.may_freeze_or_delete() {
        Some("the program may freeze or delete".into())
    } else if ast.array_prop_reads_may_raise {
        // a read past the end can reach a getter on the prototype
        // chain, whose answer no inline slot holds
        Some("an array read may run a prototype getter".into())
    } else {
        None
    };
    if let Some(reason) = global {
        return (cands.into_iter())
            .map(|c| (c, Err(reason.clone())))
            .chain(refused)
            .collect();
    }
    let prog = Program::index(ast, expr_types, dead);
    let found: Vec<_> = cands.iter().map(|c| walk::judge(&prog, c)).collect();
    cands.into_iter().zip(found).chain(refused).collect()
}

/// The sids of the admitted classes whose layout is all scalar slots
/// and is no other name's, and whose every field can take what a new
/// instance holds there ([`field_takes`]), each with those values.
pub(crate) fn value_sids(
    admitted: &[Admitted],
    aliases: &HashMap<String, ssa::Type>,
    struct_layouts: &[Vec<(PropKey, ssa::Type)>],
    ast: &Ast,
    widths: &WidthTable,
) -> HashMap<ssa::StructId, Vec<(String, FieldInit)>> {
    let mut out = HashMap::new();
    let stats = !admitted.is_empty() && stats();
    for (n, inits) in admitted {
        let Some(ssa::Type::Obj(sid)) = aliases.get(n) else {
            continue;
        };
        let verdict = layout_takes(n, inits, *sid, aliases, struct_layouts, ast, widths);
        match verdict {
            Ok(()) => {
                out.insert(*sid, inits.clone());
                if stats {
                    eprintln!("[value_class] hit: {n}");
                }
            }
            Err(reason) if stats => eprintln!("[value_class] out: {n}: {reason}"),
            Err(_) => {}
        }
    }
    out
}

fn layout_takes(
    class: &str,
    inits: &[(String, FieldInit)],
    sid: ssa::StructId,
    aliases: &HashMap<String, ssa::Type>,
    struct_layouts: &[Vec<(PropKey, ssa::Type)>],
    ast: &Ast,
    widths: &WidthTable,
) -> Result<(), String> {
    let layout = &struct_layouts[sid.0 as usize];
    if aliases
        .iter()
        .any(|(m, t)| m != class && *t == ssa::Type::Obj(sid))
    {
        return Err("another name shares the layout".into());
    }
    let factory = format!("__new_{class}");
    let params = decl::fn_params(ast, &factory).unwrap_or_default();
    for (f, slot) in layout {
        let Some((_, init)) = inits.iter().find(|(n, _)| f == n.as_str()) else {
            return Err(format!("field `{}` has no value", f.lossy()));
        };
        let param = match init {
            FieldInit::Arg(i) => params.get(*i).map(|p| param_ty(widths, &factory, p)),
            _ => None,
        };
        if !field_takes(*slot, *init, param.flatten()) {
            return Err(format!(
                "field `{}` ({slot:?}) cannot hold {init:?}",
                f.lossy()
            ));
        }
    }
    Ok(())
}

/// The slot type the factory's parameter `p` lowers to: a `number`
/// widens to a float when the width analysis says one reaches it.
fn param_ty(widths: &WidthTable, factory: &str, p: &crate::ast::Param) -> Option<ssa::Type> {
    match p.type_ann.as_deref() {
        Some("number") => {
            let key = crate::num_width::SlotKey::Param(factory.to_string(), p.name.clone());
            Some(if widths.slot_is_f64(&key) {
                ssa::Type::F64
            } else {
                ssa::Type::I64
            })
        }
        Some("boolean") => Some(ssa::Type::Bool),
        _ => None,
    }
}

/// Whether a field slot of type `slot` holds `init` as the cell's
/// field would: an argument of the slot's own type (or an integer
/// into a float slot), a number literal a float slot holds, or an
/// integer slot holds exactly (no fraction, no `-0`, inside 2^53), a
/// boolean literal into a boolean slot.
fn field_takes(slot: ssa::Type, init: FieldInit, param: Option<ssa::Type>) -> bool {
    use ssa::Type as T;
    match (slot, init) {
        (T::F64, FieldInit::Number(_)) | (T::Bool, FieldInit::Bool(_)) => true,
        (T::I64, FieldInit::Number(n)) => {
            n.fract() == 0.0 && n.abs() < (1u64 << 53) as f64 && !(n == 0.0 && n.is_sign_negative())
        }
        (slot, FieldInit::Arg(_)) => match param {
            Some(p) => p == slot || (slot == T::F64 && p == T::I64),
            None => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldInit, field_takes};
    use crate::ssa::Type;

    #[test]
    fn a_slot_takes_only_what_it_holds_exactly() {
        let num = FieldInit::Number;
        assert!(field_takes(Type::F64, num(0.5), None));
        assert!(field_takes(Type::I64, num(3.0), None));
        assert!(!field_takes(Type::I64, num(0.5), None));
        assert!(!field_takes(Type::I64, num(-0.0), None));
        assert!(!field_takes(Type::I64, num(1e16), None));
        assert!(!field_takes(Type::I32, num(1.0), None));
        assert!(field_takes(Type::Bool, FieldInit::Bool(true), None));
        assert!(!field_takes(Type::F64, FieldInit::Bool(true), None));
        let arg = FieldInit::Arg(0);
        assert!(field_takes(Type::F64, arg, Some(Type::I64)));
        assert!(field_takes(Type::I64, arg, Some(Type::I64)));
        assert!(!field_takes(Type::I64, arg, Some(Type::F64)));
        assert!(!field_takes(Type::Bool, arg, Some(Type::I64)));
        assert!(!field_takes(Type::I64, arg, None));
    }
}
