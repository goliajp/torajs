//! The statement-position rewrite of the eval desugar: `rewrite_list`
//! / `rewrite_stmt`, the walk that turns a literal `eval("…")` /
//! `(0, eval)("…")` statement into the statements its text parses to.
//! Moved verbatim out of `desugar_eval.rs` at the 500-line cap
//! (rotation 638); the parent keeps the pass entry, the value-position
//! collapse and the arrow-body walk, and calls `rewrite_list` from both.

use super::super::{Ast, Expr, Stmt};
use super::recognize::{has_use_strict_prologue, literal_eval_call};
use super::scope::{self, seal_var_scope};
use super::source::{
    CallForm, EvalStrictness, eval_strictness, first_line, parse_eval_source, syntax_error_throw,
};

pub(super) fn rewrite_list(
    stmts: &mut Vec<Stmt>,
    ast: &mut Ast,
    in_fn: bool,
    home: bool,
    strict: &[bool],
) {
    // An eval / Script inlined as a `Multi` is spliced flat into the
    // list it sits in, so a Script's `let` / `const` / `class` are the
    // list's own lexical bindings and its function declarations are
    // top-level declarations to every pass that reads the top-level
    // list (`script_global_fns`, the `var` hoist's function
    // pre-binding). Only the Multis THIS pass produced: a top-level
    // Multi also comes out of the parse-time destructuring desugar,
    // and flattening that one turned its `__dstr` temporaries into
    // top-level bindings the checker then typed a `var` from
    // (rotation 638, `assignment/dstr/obj-rest-*`).
    let mut spliced: Vec<usize> = Vec::new();
    for (i, s) in stmts.iter_mut().enumerate() {
        let was_eval = matches!(&*s, Stmt::Expr(e) if literal_eval_call(*e, ast).is_some());
        rewrite_stmt(s, ast, in_fn, home, strict);
        if was_eval && matches!(s, Stmt::Multi(_)) {
            spliced.push(i);
        }
    }
    for i in spliced.into_iter().rev() {
        if let Stmt::Multi(v) = std::mem::replace(&mut stmts[i], Stmt::Multi(Vec::new())) {
            stmts.splice(i..=i, super::script_form::flatten_top_multi(v));
        }
    }
}

fn rewrite_stmt(s: &mut Stmt, ast: &mut Ast, in_fn: bool, home: bool, strict: &[bool]) {
    // The rewrite itself: a statement that is nothing but a call to
    // `eval` with one literal argument becomes the block that literal
    // parses to. A direct eval's block is sealed (strict — nothing
    // escapes); an indirect eval's block is not (sloppy global code —
    // `var` hoists out, `let` stays), and is only built at the top
    // level, where the surrounding scope IS the global scope. An
    // indirect statement inside a function is left alone rather than
    // wrongly attached to the function's scope.
    if let Stmt::Expr(eid) = s {
        if let Some((src, form)) = literal_eval_call(*eid, ast) {
            let inline_here = form == CallForm::Direct || !in_fn;
            let sw = eval_strictness(form, eid.0 as usize, strict);
            match parse_eval_source(&src, ast, form == CallForm::Direct && home, sw) {
                Ok(mut inlined) if inline_here => {
                    // An eval inside the inlined text is an eval like
                    // any other; the nesting is finite because each
                    // level is a literal written in the level above.
                    rewrite_list(&mut inlined, ast, in_fn, home, strict);
                    // §19.2.1.1 steps 3-5 seal on STRICTNESS, not on
                    // call form: strict eval code gets its own
                    // VariableEnvironment and its `var`s die with the
                    // eval, while a SLOPPY direct eval's `var` lands in
                    // the caller's. `sw` is that verdict; a "use
                    // strict" prologue in the text arms it from the
                    // other side, which is how an indirect eval — never
                    // strict by inheritance — can still seal.
                    // A Script's `var`s are global whatever its
                    // strictness (§16.1.7 has no strict branch).
                    let prologue = has_use_strict_prologue(&inlined, ast);
                    let sealed =
                        form != CallForm::Script && (sw == EvalStrictness::Strict || prologue);
                    // The prologue makes the text strict code under a
                    // sloppy caller (§11.2.2), and strictness is read
                    // off the enclosing function, so the text gets a
                    // function of its own. Strict eval code owns its
                    // var scope and cannot `return` or `break` out, and
                    // an arrow keeps the caller's `this` / `arguments`.
                    if sealed && prologue && sw != EvalStrictness::Strict {
                        seal_var_scope(&mut inlined);
                        let arrow = ast.add_expr(Expr::ArrowFn {
                            params: Vec::new(),
                            return_type: Some("void".to_string()),
                            body: inlined,
                        });
                        ast.exprs[eid.0 as usize] = Expr::Call {
                            callee: arrow,
                            args: Vec::new(),
                        };
                        return;
                    }
                    if sealed {
                        seal_var_scope(&mut inlined);
                    } else {
                        if form != CallForm::Script {
                            super::script_form::drop_script_shadowed_decls(&mut inlined, ast);
                        }
                        if !in_fn {
                            super::instantiate::prepend_global_instantiation(&mut inlined, ast);
                        }
                        super::script_form::record_eval_var_sites(&inlined, ast, form);
                    }
                    // Blank the consumed Call: an orphan still shaped
                    // like an eval call would get an IIFE nobody
                    // constructs from the arena-walking completion
                    // pass (r333 harness-wide lifting break).
                    ast.exprs[eid.0 as usize] = Expr::Ident("undefined".to_string());
                    *s = if sealed {
                        Stmt::Block(inlined)
                    } else if form == CallForm::Script {
                        // Flat: a Script's `let` / `const` / `class` are
                        // global lexical bindings (`script_form`).
                        Stmt::Multi(inlined)
                    } else {
                        scope::hoist_fn_decls_out(inlined)
                    };
                    return;
                }
                Ok(_) => {}
                Err(_) => {
                    // The text does not parse. §19.2.1.1 step 12 wants
                    // a SyntaxError at evaluation time — see
                    // `syntax_error_throw` on why this is a throw and
                    // not a compile error. Scope-independent, so it
                    // applies at any depth for both call forms.
                    ast.exprs[eid.0 as usize] = Expr::Ident("undefined".to_string());
                    *s = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                    return;
                }
            }
        }
        // `v = eval("(((")` — an assignment statement whose rhs is an
        // eval of a non-parsing source. §13.15.2 evaluates the target
        // reference first and the rhs second, so with a bare-identifier
        // target (no effects) the statement's entire behaviour is the
        // rhs's SyntaxError; the assignment never completes and the
        // statement becomes the throw. A member target could run a
        // getter on its object expression first, so only the identifier
        // shape rewrites.
        if let Some(Expr::Assign { target, value }) = ast.exprs.get(eid.0 as usize) {
            let (target, value) = (*target, *value);
            if matches!(ast.exprs.get(target.0 as usize), Some(Expr::Ident(_)))
                && let Some((src, form)) = literal_eval_call(value, ast)
            {
                let sw = eval_strictness(form, value.0 as usize, strict);
                if parse_eval_source(&src, ast, form == CallForm::Direct && home, sw).is_err() {
                    // Same orphan story as the inline above.
                    ast.exprs[value.0 as usize] = Expr::Ident("undefined".to_string());
                    *s = syntax_error_throw(format!("eval: {}", first_line(&src)), ast);
                    return;
                }
            }
        }
    }
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => rewrite_list(b, ast, in_fn, home, strict),
        // An ordinary function body cuts the home chain (§19.2.1.1
        // step 6 — its this-environment has no [[HomeObject]]).
        Stmt::FnDecl { body, .. } => rewrite_list(body, ast, true, false, strict),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            rewrite_stmt(then_branch, ast, in_fn, home, strict);
            if let Some(e) = else_branch.as_deref_mut() {
                rewrite_stmt(e, ast, in_fn, home, strict);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_stmt(body, ast, in_fn, home, strict)
        }
        Stmt::Labeled { body, .. } => rewrite_stmt(body, ast, in_fn, home, strict),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_stmt(i, ast, in_fn, home, strict);
            }
            rewrite_stmt(body, ast, in_fn, home, strict);
        }
        Stmt::ForOf { body, .. } => rewrite_stmt(body, ast, in_fn, home, strict),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            rewrite_list(body, ast, in_fn, home, strict);
            rewrite_list(catch_body, ast, in_fn, home, strict);
            if let Some(f) = finally_body.as_mut() {
                rewrite_list(f, ast, in_fn, home, strict);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_list(&mut c.body, ast, in_fn, home, strict);
            }
            if let Some(d) = default.as_mut() {
                rewrite_list(d, ast, in_fn, home, strict);
            }
        }
        _ => {}
    }
}
