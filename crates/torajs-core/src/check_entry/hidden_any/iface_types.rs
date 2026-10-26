//! The types the witness table matches a receiver against
//! (`IfaceSlots::types`), retyped with the interfaces whose methods a
//! round retypes to return `any`.
//!
//! An interface is matched by type equality, nested members included:
//! a type spelling a retyped interface's shape in a member
//! (`i: { m(): number }`) is respelled with it
//! (`super::method_lits`), and its own type has to follow, or no
//! declaration is equal to the respelled literals any more.

use std::collections::HashMap;

use super::method_rets::Family;
use super::type_parts::each_part_mut;
use crate::ast::Ast;
use crate::check::Type;

/// Retype the members of `fam.members` to return `any` in the types of
/// their interfaces, and every type nested in a member of an interface
/// that is one of the retyped types as it was.
pub(super) fn retype(r: &mut Ast, fam: &Family) {
    if fam.members.is_empty() {
        return;
    }
    let types = &mut r.iface_slots.types;
    let before: HashMap<String, Type> = types.clone();
    for (i, k) in &fam.members {
        if let Some(Type::Struct(fs)) = types.get_mut(i) {
            for (_, t) in fs.iter_mut().filter(|(n, _)| n == k) {
                match t {
                    Type::Function(_, ret) => **ret = Type::Any,
                    Type::Nullable(f) => {
                        if let Type::Function(_, ret) = &mut **f {
                            **ret = Type::Any;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    // each pass reaches one level deeper, and there are no more levels
    // than interfaces
    for _ in 0..before.len() {
        let moved: Vec<(Type, Type)> = (before.iter())
            .filter_map(|(i, was)| Some((was, types.get(i)?)))
            .filter(|(was, now)| was != now)
            .map(|(was, now)| (was.clone(), now.clone()))
            .collect();
        let mut changed = false;
        for t in types.values_mut() {
            if let Type::Struct(fs) = t {
                for (_, f) in fs.iter_mut() {
                    changed |= follow(f, &moved);
                }
            }
        }
        if !changed {
            break;
        }
    }
}

/// Replace in `t` each type that is the first of a pair of `moved` by
/// the second; true when one was replaced.
fn follow(t: &mut Type, moved: &[(Type, Type)]) -> bool {
    if let Some((_, now)) = moved.iter().find(|(was, _)| was == t) {
        *t = now.clone();
        return true;
    }
    let mut changed = false;
    each_part_mut(t, |p| changed |= follow(p, moved));
    changed
}
