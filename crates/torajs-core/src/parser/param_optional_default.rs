//! The implicit default an optional parameter (`name?: T`) binds
//! when a call site omits it — split from `param_list.rs` at the
//! size cap (rotation 227's watch: the file sat 3 lines under 500).
//!
//! ES §9.2: an absent optional binds undefined. An `any` /
//! un-annotated slot mints the real undefined ident (`typeof y ===
//! "undefined"` answers per spec); a TYPED optional keeps the
//! legacy implicit null — its `__nullable(T)` slot has no
//! undefined arm (registered). Shared by parse_fn, the ctor param
//! list, and parse_param_list (methods / fn-exprs).

use super::Parser;
use crate::ast::{Expr, ExprId};

impl<'a> Parser<'a> {
    pub(super) fn implicit_optional_default(&mut self, type_ann: Option<&str>) -> ExprId {
        let undef_ok = type_ann.is_none_or(|a| a == "__nullable(any)" || a == "any");
        if undef_ok {
            self.ast.add_expr(Expr::Ident("undefined".into()))
        } else {
            self.ast.add_expr(Expr::Null)
        }
    }
}
