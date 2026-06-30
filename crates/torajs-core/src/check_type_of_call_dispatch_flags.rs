//! Per-call-site dispatch flag derivation extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 306 — ninety-eighth
//! sub-batch of check_type_of_call.rs per-shape
//! decomposition).
//!
//! **M5.1 Class method**: `__cm_C__m(receiver, ...)` form
//! (matched by `is_class_method_name`) borrows the
//! receiver. Arg[0] is read, never consumed; args[1..]
//! follow the normal affine rules. The flag also unlocks
//! the M5.2 subclass-receiver skip in the type-check loop
//! (structural prefix sub-typing).
//!
//! Originally returned a `(is_string_borrow,
//! is_class_method)` tuple — the M6.1 String borrow flag
//! was derived for future ssa-lower-side RC-drop elision
//! use but never consumed by the host or any downstream
//! site (chunk 311 drop). If a future M6.1 wire-back ever
//! needs the predicate, the call-site re-derivation lives
//! one matches! away from [`crate::check::STRING_BORROW_METHODS`].
//!
//! Pure read-only derivation; no mutation.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::is_class_method_name;

pub(crate) fn derive(ast: &Ast, callee: &ExprId) -> bool {
    matches!(
        ast.get_expr(*callee),
        Expr::Ident(name) if is_class_method_name(name)
    )
}
