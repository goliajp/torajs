//! Per-call-site consume bitmap derivation extracted from
//! [`crate::check_type_of_call::check`]'s post-cascade
//! generic-call mechanics (chunk 305 — ninety-seventh
//! sub-batch of check_type_of_call.rs per-shape
//! decomposition).
//!
//! Per the M5.x ownership model: each declared param of a
//! callee fn is either *consuming* (the body flows the
//! arg into `__new_*` / `this.<field> =` sinks → callee
//! transfers ownership) or *borrow* (callee reads but
//! never consumes). `compute_consuming_params` walks every
//! fn body once and caches the per-param bitmap on
//! `ast.consuming_params`; we look it up here at the call
//! site.
//!
//! Fallback policy:
//! - Known callee (`Expr::Ident(name)` with cached entry):
//!   use the cached bitmap as-is.
//! - `__new_*` synthetic constructor-factory ident
//!   (intrinsic that always consumes every arg into the
//!   struct slot): all-true bitmap.
//! - Unknown callee (e.g. intrinsic without a cached
//!   entry, Member dispatch, computed callee): all-false
//!   bitmap (default borrow).
//!
//! Pure read-only derivation: returns a fresh `Vec<bool>`
//! of length `args_len`; no mutation of ast / checker /
//! params / args.

use crate::ast::{Ast, Expr, ExprId};

pub(crate) fn derive(ast: &Ast, callee: &ExprId, args_len: usize) -> Vec<bool> {
    match ast.get_expr(*callee) {
        Expr::Ident(callee_name) => {
            if let Some(bm) = ast.consuming_params.get(callee_name) {
                bm.clone()
            } else if callee_name.starts_with("__new_") {
                vec![true; args_len]
            } else {
                vec![false; args_len]
            }
        }
        _ => vec![false; args_len],
    }
}
