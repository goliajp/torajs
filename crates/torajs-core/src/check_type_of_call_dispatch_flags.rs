//! Per-call-site dispatch flag derivations extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 306 — ninety-eighth
//! sub-batch of check_type_of_call.rs per-shape
//! decomposition).
//!
//! Two boolean flags derived once per call site, consumed
//! by the per-arg loop below for type-check skip / consume
//! decisions:
//!
//! - **M6.1 String borrow**: `s.slice` / `.includes` /
//!   `.indexOf` and the rest of [`STRING_BORROW_METHODS`]
//!   don't transfer ownership — they read both receiver
//!   and args, allocate a fresh result, and return. The
//!   per-arg consume bitmap derived in chunk 305 stays at
//!   default-false for these; the flag suppresses the
//!   M6.1 RC-drop emit path downstream.
//!
//! - **M5.1 Class method**: `__cm_C__m(receiver, ...)`
//!   form (matched by `is_class_method_name`) borrows
//!   the receiver. Arg[0] is read, never consumed;
//!   args[1..] follow the normal affine rules. The flag
//!   also unlocks the M5.2 subclass-receiver skip in the
//!   type-check loop (structural prefix sub-typing).
//!
//! Pure read-only derivation: returns `(is_string_borrow,
//! is_class_method)` tuple; no mutation.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{STRING_BORROW_METHODS, is_class_method_name};

pub(crate) fn derive(ast: &Ast, callee: &ExprId) -> (bool, bool) {
    let is_string_borrow = matches!(
        ast.get_expr(*callee),
        Expr::Member { obj: _, name }
            if STRING_BORROW_METHODS.iter().any(|m| *m == name.as_str())
    );
    let is_class_method = matches!(
        ast.get_expr(*callee),
        Expr::Ident(name) if is_class_method_name(name)
    );
    (is_string_borrow, is_class_method)
}
