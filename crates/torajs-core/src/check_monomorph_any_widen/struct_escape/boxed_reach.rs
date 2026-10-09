//! Every function a boxed-entry call with an `any` argument may reach
//! has to hold its twin.
//!
//! `g(a)` with `a: any` for an object parameter of the function value
//! `g` rides the value's boxed entry
//! (`check_type_of_call::any_struct_slot`): the adapter hands an
//! argument the typed body cannot read to the all-`any` twin. A
//! closure that took no twin — a generic or generator target, an argv
//! or receiver channel, a body that writes a container-typed binding
//! outside it ([`super::closure`]) — leaves the adapter only the
//! coercion kernel, which reads a `5` or `undefined` as a cell. So such
//! a closure refuses the program, as the call was refused before.
//!
//! What `g` may hold is read off the closures that can leave typed
//! calls at all (the loose ones, which a value binding needs) and
//! declare the same parameter types: a closure of another face cannot
//! be stored there.

use std::collections::HashSet;

use super::closure::loose_values;
use super::twin_name;
use crate::ast::Ast;
use crate::check::{Checker, DiagPush, Type};

/// Refuse the program for each loose closure or forwarder shell of a
/// recorded face without a twin. `retired` are the generic originals,
/// whose bodies never lower.
pub(crate) fn refuse_untwinned(c: &mut Checker, ast: &Ast, retired: &HashSet<String>) {
    if c.any_struct_arg_callees.is_empty() {
        return;
    }
    let mut reach = loose_values(c, ast, "__closure_");
    reach.extend(loose_values(c, ast, "__forward_"));
    for name in reach {
        if retired.contains(&name) || c.globals.contains_key(&twin_name(&name)) {
            continue;
        }
        let Some(Type::Function(params, _)) = c.globals.get(&name) else {
            continue;
        };
        let Some((callee, _)) = c
            .any_struct_arg_callees
            .iter()
            .find(|(_, face)| *face == params)
        else {
            continue;
        };
        let msg = format!(
            "`{callee}` is called with an `any` argument for an object parameter, and a \
             function it may hold cannot take one"
        );
        c.errors.push_err(msg);
    }
}
