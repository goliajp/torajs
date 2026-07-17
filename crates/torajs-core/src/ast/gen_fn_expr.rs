//! Generator function-expression markers — carved out of `ast.rs`
//! (file-size hard limit) when RFC 20260717-fnexpr-this-channel knife
//! 1 grew the marker-registry face past 500 prod lines. Payload types
//! for `Ast::gen_fn_exprs`; the set itself stays on the `Ast` struct.

/// Which function-value expression form a `gen_fn_exprs` entry came
/// from. Only the two generator shapes hoist: plain `async
/// function(){}` expressions stay `Expr::ArrowFn` and ride the
/// closure lift (marked in `async_fn_value_exprs` instead), so no
/// `Async` variant exists here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenFnExprKind {
    Generator,
    AsyncGenerator,
}

/// Per-expression payload for `Ast::gen_fn_exprs`: the form kind plus
/// how many parser-synthesized destructuring lets prefix the body
/// (mirrors `gen_param_destr_prefix` for decl-form generators —
/// `hoist_gen_fn_exprs` re-registers the count under the hoisted name
/// so the __Gen ctor gets the eager destructure).
#[derive(Debug, Clone, Copy)]
pub struct GenFnExprInfo {
    pub kind: GenFnExprKind,
    pub destr_prefix: usize,
}
