//! A Script evaluated INSIDE a function — `$262.evalScript(src)` in
//! a function body whose text declares something the program does
//! not (RFC 20260912-global-var-cell knife 5).
//!
//! §16.1.7 GlobalDeclarationInstantiation runs when the Script is
//! evaluated, against the global environment as it is THEN: whether
//! `var x` may be created depends on `Object.preventExtensions(this)`
//! having run, whether `function data1() {}` may depends on what
//! `defineProperty(this, "data1", …)` left there, and a `let` over a
//! non-configurable property is a SyntaxError — none of which the
//! compiler can see (`language/global-code/script-decl-*-err`,
//! `script-decl-lex-restricted-global`). And the text is GLOBAL code:
//! a free name in it resolves against the global environment, never
//! against the calling function's locals.
//!
//! So the call becomes a call to a synthesized TOP-LEVEL function
//! `__script_<id>` — top-level, so its free names see only the
//! program's globals — whose body is, in the spec's order:
//!
//! 1. one `__torajs_script_preflight_{lex,fn,var}(name)` per declared
//!    name (steps 5.c, 8-10, 12): ALL the checks before ANY binding,
//!    so a refused Script creates nothing;
//! 2. the text's function declarations (kept as nested declarations,
//!    hoisted to the front so their values exist before anything
//!    runs), each handed to `__torajs_global_fn_declare(name, f)`
//!    (step 16);
//! 3. one `__torajs_global_var_declare(name)` per `var` name (step 17);
//! 4. the text, its `var` declarations turned into the assignments
//!    they are once the binding exists.
//!
//! The bindings are plain properties of the global object — there is
//! no compiled slot for a cell to point at — and every use of the
//! name, in the text or after it, is the unresolvable-name lane
//! (`globalthis_object_idents`). Registered residue: a lexical
//! declaration (`let` / `const` / `class`) in such a Script has no
//! global lexical record to land in; after the preflight (which is
//! what `script-decl-lex-restricted-global` asks) it is refused
//! loudly. A `for (var k …)` head's `k` stays the loop's own binding.

use super::super::{Ast, Expr, ExprId, Stmt};
use super::script_form::keep_last_fn_decls;

/// The synthesized function's name — unique per call site (the call
/// expression's arena id), and a `__` name `is_synthesized_dunder`
/// keeps off the global object.
fn script_fn_name(call: ExprId) -> String {
    format!("__script_{}", call.0)
}

/// The names one preflight / declare call carries, in §16.1.7's order.
pub(super) struct ScriptNames {
    pub lex: Vec<String>,
    pub fns: Vec<String>,
    pub vars: Vec<String>,
}

/// Build the top-level function for the Script at `call` and return
/// the statement that replaces the call: `__script_<id>()`.
pub(super) fn synthesize(
    mut parsed: Vec<Stmt>,
    names: ScriptNames,
    call: ExprId,
    ast: &mut Ast,
    sink: &mut Vec<Stmt>,
) -> Stmt {
    keep_last_fn_decls(&mut parsed);
    let strict_prologue = super::recognize::has_use_strict_prologue(&parsed, ast);
    let mut body: Vec<Stmt> = Vec::with_capacity(parsed.len() + 8);
    if strict_prologue {
        body.push(Stmt::Expr(ast.add_expr(Expr::String("use strict".into()))));
    }
    for n in &names.lex {
        body.push(synthetic_call(
            ast,
            "__torajs_script_preflight_lex",
            n,
            None,
        ));
    }
    for n in &names.fns {
        body.push(synthetic_call(ast, "__torajs_script_preflight_fn", n, None));
    }
    for n in &names.vars {
        body.push(synthetic_call(
            ast,
            "__torajs_script_preflight_var",
            n,
            None,
        ));
    }
    if let Some(n) = names.lex.first() {
        body.push(super::source::syntax_error_throw(
            format!("torajs: a lexical declaration (`{n}`) in a Script evaluated inside a function is not yet supported"),
            ast,
        ));
    }
    let (decls, rest): (Vec<Stmt>, Vec<Stmt>) = parsed
        .into_iter()
        .partition(|s| matches!(s, Stmt::FnDecl { .. }));
    body.extend(decls);
    for n in &names.fns {
        let value = ast.add_expr(Expr::Ident(n.clone()));
        body.push(synthetic_call(
            ast,
            "__torajs_global_fn_declare",
            n,
            Some(value),
        ));
    }
    for n in &names.vars {
        body.push(synthetic_call(ast, "__torajs_global_var_declare", n, None));
    }
    let mut rest = rest;
    for s in rest.iter_mut() {
        var_to_assign(s, ast);
    }
    body.extend(rest);
    let name = script_fn_name(call);
    sink.push(Stmt::FnDecl {
        name: name.clone(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: None,
        body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    let callee = ast.add_expr(Expr::Ident(name));
    Stmt::Expr(ast.add_expr(Expr::Call {
        callee,
        args: Vec::new(),
    }))
}

/// `callee("name")` / `callee("name", value)` as a statement.
fn synthetic_call(ast: &mut Ast, callee: &str, name: &str, value: Option<ExprId>) -> Stmt {
    let callee = ast.add_expr(Expr::Ident(callee.to_string()));
    let mut args = vec![ast.add_expr(Expr::String(name.into()))];
    args.extend(value);
    Stmt::Expr(ast.add_expr(Expr::Call { callee, args }))
}

/// `var x = e` → `x = e` (the binding exists by the time the text
/// runs); `var x;` → nothing. Walks the nesting the `var` hoist
/// counts (`count_var_names_stmt`), stopping at a nested function,
/// whose `var`s are its own.
fn var_to_assign(s: &mut Stmt, ast: &mut Ast) {
    if let Stmt::LetDecl {
        name,
        init,
        is_var: true,
        ..
    } = s
    {
        *s = if matches!(ast.get_expr(*init), Expr::Uninit) {
            Stmt::Expr(ast.add_expr(Expr::Ident("undefined".to_string())))
        } else {
            let target = ast.add_expr(Expr::Ident(name.clone()));
            let value = *init;
            Stmt::Expr(ast.add_expr(Expr::Assign { target, value }))
        };
        return;
    }
    if matches!(s, Stmt::FnDecl { .. }) {
        return;
    }
    for c in super::super::desugar_with::walk::stmt_children(s) {
        var_to_assign(c, ast);
    }
}
