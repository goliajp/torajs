//! AST pretty-printer extracted from [`crate::ast`] (chunk 141).
//!
//! `print_stmt` was 291 LOC inline as a method on `Ast` (over the
//! 200-line god-fn hard limit per `torajs-file-size-debt`).
//! `print_expr` was a paired 142 LOC method; extracted together
//! since they recurse into each other. Free-fn shape with
//! `ast: &Ast` borrow makes recursion plain calls without going
//! back through a method dispatch.
//!
//! Caller in `ast.rs`'s `pub fn print(&self)` is now a 3-line
//! wrapper looping `print_stmt(self, s, 0)` per top-level
//! statement.
//!
//! Chunk 454: fat arms (let / switch / for / try / class) extracted
//! into per-arm fns, the 4× repeated param-list formatting deduped
//! into `fmt_params`, and `print_expr` moved to the `expr` child
//! module to keep both halves under the 500-line file limit.

mod expr;

pub(crate) use expr::print_expr;

use crate::ast::{Ast, ExprId, Param, Stmt};

/// `name: ann` comma list shared by FnDecl / ctor / method / ArrowFn
fn fmt_params(params: &[Param]) -> String {
    params
        .iter()
        .map(|p| match &p.type_ann {
            Some(t) => format!("{}: {t}", p.name),
            None => p.name.clone(),
        })
        .collect::<Vec<String>>()
        .join(", ")
}

fn fmt_type_params(type_params: &[String]) -> String {
    if type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", type_params.join(", "))
    }
}

/// `Stmt::Break` / `Stmt::Continue` / `Stmt::Labeled` debug-print arms,
/// split out of `print_stmt` to keep that dispatcher within the function
/// size limit.
fn print_jump(ast: &Ast, s: &Stmt, indent: usize) {
    let pad = "  ".repeat(indent);
    match s {
        Stmt::Break(label) => match label {
            Some(l) => println!("{pad}Break {l}"),
            None => println!("{pad}Break"),
        },
        Stmt::Continue(label) => match label {
            Some(l) => println!("{pad}Continue {l}"),
            None => println!("{pad}Continue"),
        },
        Stmt::Labeled { label, body } => {
            println!("{pad}Labeled {label}");
            print_stmt(ast, body, indent + 1);
        }
        _ => {}
    }
}

pub(crate) fn print_stmt(ast: &Ast, s: &Stmt, indent: usize) {
    let pad = "  ".repeat(indent);
    match s {
        Stmt::Expr(eid) => {
            println!("{pad}ExprStmt");
            print_expr(ast, *eid, indent + 1);
        }
        Stmt::Yield(eid) => {
            println!("{pad}Yield");
            print_expr(ast, *eid, indent + 1);
        }
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            println!("{pad}YieldInto var={var} ty={type_ann:?}");
            print_expr(ast, *value, indent + 1);
        }
        Stmt::UsingDecl {
            name,
            type_ann,
            init,
            is_await,
        } => {
            println!("{pad}UsingDecl {name} ty={type_ann:?} await={is_await}");
            print_expr(ast, *init, indent + 1);
        }
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            is_var,
        } => print_let_decl(
            ast,
            &pad,
            *mutable,
            *is_var,
            name,
            type_ann.as_deref(),
            *init,
            indent,
        ),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => print_if(
            ast,
            &pad,
            *cond,
            then_branch,
            else_branch.as_deref(),
            indent,
        ),
        Stmt::While { cond, body } => {
            println!("{pad}While");
            println!("{pad}  cond:");
            print_expr(ast, *cond, indent + 2);
            println!("{pad}  body:");
            print_stmt(ast, body, indent + 2);
        }
        Stmt::DoWhile { body, cond } => {
            println!("{pad}DoWhile");
            println!("{pad}  body:");
            print_stmt(ast, body, indent + 2);
            println!("{pad}  cond:");
            print_expr(ast, *cond, indent + 2);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => print_switch(ast, &pad, *scrutinee, cases, default.as_deref(), indent),
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => print_for(ast, &pad, init.as_deref(), *cond, *step, body, indent),
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::Labeled { .. } => print_jump(ast, s, indent),
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => print_for_of_split(ast, &pad, var_name, *parent, *sep, body, indent),
        Stmt::ForOf {
            var_name,
            src_ident,
            elem_expr,
            body,
            ..
        } => print_for_of(ast, &pad, var_name, src_ident, *elem_expr, body, indent),
        Stmt::Throw(eid) => {
            println!("{pad}Throw");
            print_expr(ast, *eid, indent + 1);
        }
        Stmt::Try {
            body,
            catch_param,
            catch_type: _,
            had_catch: _,
            catch_body,
            finally_body,
        } => print_try(
            ast,
            &pad,
            body,
            catch_param.as_deref(),
            catch_body,
            finally_body.as_deref(),
            indent,
        ),
        Stmt::Block(stmts) => {
            println!("{pad}Block");
            for s in stmts {
                print_stmt(ast, s, indent + 1);
            }
        }
        Stmt::Multi(stmts) => {
            println!("{pad}Multi");
            for s in stmts {
                print_stmt(ast, s, indent + 1);
            }
        }
        Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type,
            body,
            is_generator: _,
            span: _,
        } => print_fn_decl(
            ast,
            &pad,
            name,
            type_params,
            params,
            return_type,
            body,
            indent,
        ),
        Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } => {
            let parts: Vec<String> = fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            println!(
                "{pad}TypeDecl {name}{} = {{ {} }}",
                fmt_type_params(type_params),
                parts.join(", ")
            );
        }
        Stmt::Return(maybe) => match maybe {
            Some(eid) => {
                println!("{pad}Return");
                print_expr(ast, *eid, indent + 1);
            }
            None => println!("{pad}Return"),
        },
        Stmt::ClassDecl {
            name,
            type_params: _,
            parent,
            is_abstract: _,
            fields,
            static_init: _,
            ctor,
            methods,
            static_methods: _,
        } => print_class_decl(
            ast,
            &pad,
            name,
            parent.as_deref(),
            fields,
            ctor,
            methods,
            indent,
        ),
        Stmt::ImportDecl { source, .. } => {
            println!("{pad}ImportDecl {source:?}");
        }
        Stmt::ExportDecl { inner, .. } => {
            println!("{pad}ExportDecl");
            if let Some(inner) = inner {
                print_stmt(ast, inner, indent + 1);
            }
        }
    }
}

/// `Stmt::ForOfSplitIter` arm — parent / sep / body, each one indent
/// deeper.
fn print_for_of_split(
    ast: &Ast,
    pad: &str,
    var_name: &str,
    parent: ExprId,
    sep: ExprId,
    body: &Stmt,
    indent: usize,
) {
    println!("{pad}ForOfSplitIter {var_name}");
    println!("{pad}  parent:");
    print_expr(ast, parent, indent + 2);
    println!("{pad}  sep:");
    print_expr(ast, sep, indent + 2);
    println!("{pad}  body:");
    print_stmt(ast, body, indent + 2);
}

/// `Stmt::ForOf` arm — elem expr + body.
fn print_for_of(
    ast: &Ast,
    pad: &str,
    var_name: &str,
    src_ident: &str,
    elem_expr: ExprId,
    body: &Stmt,
    indent: usize,
) {
    println!("{pad}ForOf {var_name} of {src_ident}[i]");
    println!("{pad}  elem:");
    print_expr(ast, elem_expr, indent + 2);
    println!("{pad}  body:");
    print_stmt(ast, body, indent + 2);
}

/// `Stmt::If` arm — cond / then / optional else, each one indent
/// deeper.
fn print_if(
    ast: &Ast,
    pad: &str,
    cond: ExprId,
    then_branch: &Stmt,
    else_branch: Option<&Stmt>,
    indent: usize,
) {
    println!("{pad}If");
    println!("{pad}  cond:");
    print_expr(ast, cond, indent + 2);
    println!("{pad}  then:");
    print_stmt(ast, then_branch, indent + 2);
    if let Some(eb) = else_branch {
        println!("{pad}  else:");
        print_stmt(ast, eb, indent + 2);
    }
}

#[allow(clippy::too_many_arguments)]
fn print_let_decl(
    ast: &Ast,
    pad: &str,
    mutable: bool,
    is_var: bool,
    name: &str,
    type_ann: Option<&str>,
    init: ExprId,
    indent: usize,
) {
    let kw = if is_var {
        "var"
    } else if mutable {
        "let"
    } else {
        "const"
    };
    match type_ann {
        Some(ann) => println!("{pad}{kw} {name}: {ann}"),
        None => println!("{pad}{kw} {name}"),
    }
    print_expr(ast, init, indent + 1);
}

fn print_switch(
    ast: &Ast,
    pad: &str,
    scrutinee: ExprId,
    cases: &[crate::ast::SwitchCase],
    default: Option<&[Stmt]>,
    indent: usize,
) {
    println!("{pad}Switch");
    println!("{pad}  on:");
    print_expr(ast, scrutinee, indent + 2);
    for c in cases {
        println!("{pad}  case:");
        print_expr(ast, c.value, indent + 2);
        for s in &c.body {
            print_stmt(ast, s, indent + 2);
        }
    }
    if let Some(db) = default {
        println!("{pad}  default:");
        for s in db {
            print_stmt(ast, s, indent + 2);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn print_for(
    ast: &Ast,
    pad: &str,
    init: Option<&Stmt>,
    cond: Option<ExprId>,
    step: Option<ExprId>,
    body: &Stmt,
    indent: usize,
) {
    println!("{pad}For");
    if let Some(i) = init {
        println!("{pad}  init:");
        print_stmt(ast, i, indent + 2);
    }
    if let Some(c) = cond {
        println!("{pad}  cond:");
        print_expr(ast, c, indent + 2);
    }
    if let Some(st) = step {
        println!("{pad}  step:");
        print_expr(ast, st, indent + 2);
    }
    println!("{pad}  body:");
    print_stmt(ast, body, indent + 2);
}

#[allow(clippy::too_many_arguments)]
fn print_try(
    ast: &Ast,
    pad: &str,
    body: &[Stmt],
    catch_param: Option<&str>,
    catch_body: &[Stmt],
    finally_body: Option<&[Stmt]>,
    indent: usize,
) {
    println!("{pad}Try");
    println!("{pad}  body:");
    for s in body {
        print_stmt(ast, s, indent + 2);
    }
    if let Some(p) = catch_param {
        println!("{pad}  catch ({p}):");
    } else {
        println!("{pad}  catch:");
    }
    for s in catch_body {
        print_stmt(ast, s, indent + 2);
    }
    if let Some(fb) = finally_body {
        println!("{pad}  finally:");
        for s in fb {
            print_stmt(ast, s, indent + 2);
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// `Stmt::FnDecl` arm — header line (name + type params + params +
/// return type) then the body stmts one indent deeper.
#[allow(clippy::too_many_arguments)]
fn print_fn_decl(
    ast: &Ast,
    pad: &str,
    name: &str,
    type_params: &[String],
    params: &[Param],
    return_type: &Option<String>,
    body: &[Stmt],
    indent: usize,
) {
    let ret = return_type.clone().unwrap_or_else(|| "void".into());
    println!(
        "{pad}FnDecl {name}{}({}): {ret}",
        fmt_type_params(type_params),
        fmt_params(params)
    );
    for s in body {
        print_stmt(ast, s, indent + 1);
    }
}

fn print_class_decl(
    ast: &Ast,
    pad: &str,
    name: &str,
    parent: Option<&str>,
    fields: &[(String, String)],
    ctor: &Option<crate::ast::ClassCtor>,
    methods: &[crate::ast::ClassMethod],
    indent: usize,
) {
    let parts: Vec<String> = fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
    let ext = match parent {
        Some(p) => format!(" extends {p}"),
        None => String::new(),
    };
    println!(
        "{pad}ClassDecl {name}{ext} fields={{ {} }}",
        parts.join(", ")
    );
    if let Some(c) = ctor {
        println!("{pad}  constructor({})", fmt_params(&c.params));
        for s in &c.body {
            print_stmt(ast, s, indent + 2);
        }
    }
    for m in methods {
        let ret = m.return_type.clone().unwrap_or_else(|| "void".into());
        println!("{pad}  method {}({}): {ret}", m.name, fmt_params(&m.params));
        for s in &m.body {
            print_stmt(ast, s, indent + 2);
        }
    }
}
