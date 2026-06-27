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

use crate::ast::{Ast, Expr, ExprId, Stmt};

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
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            is_var,
        } => {
            let kw = if *is_var {
                "var"
            } else if *mutable {
                "let"
            } else {
                "const"
            };
            match type_ann {
                Some(ann) => println!("{pad}{kw} {name}: {ann}"),
                None => println!("{pad}{kw} {name}"),
            }
            print_expr(ast, *init, indent + 1);
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            println!("{pad}If");
            println!("{pad}  cond:");
            print_expr(ast, *cond, indent + 2);
            println!("{pad}  then:");
            print_stmt(ast, then_branch, indent + 2);
            if let Some(eb) = else_branch {
                println!("{pad}  else:");
                print_stmt(ast, eb, indent + 2);
            }
        }
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
        } => {
            println!("{pad}Switch");
            println!("{pad}  on:");
            print_expr(ast, *scrutinee, indent + 2);
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
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            println!("{pad}For");
            if let Some(i) = init {
                println!("{pad}  init:");
                print_stmt(ast, i, indent + 2);
            }
            if let Some(c) = cond {
                println!("{pad}  cond:");
                print_expr(ast, *c, indent + 2);
            }
            if let Some(st) = step {
                println!("{pad}  step:");
                print_expr(ast, *st, indent + 2);
            }
            println!("{pad}  body:");
            print_stmt(ast, body, indent + 2);
        }
        Stmt::Break => println!("{pad}Break"),
        Stmt::Continue => println!("{pad}Continue"),
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => {
            println!("{pad}ForOfSplitIter {var_name}");
            println!("{pad}  parent:");
            print_expr(ast, *parent, indent + 2);
            println!("{pad}  sep:");
            print_expr(ast, *sep, indent + 2);
            println!("{pad}  body:");
            print_stmt(ast, body, indent + 2);
        }
        Stmt::ForOf {
            var_name,
            src_ident,
            elem_expr,
            body,
            ..
        } => {
            println!("{pad}ForOf {var_name} of {src_ident}[i]");
            println!("{pad}  elem:");
            print_expr(ast, *elem_expr, indent + 2);
            println!("{pad}  body:");
            print_stmt(ast, body, indent + 2);
        }
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
        } => {
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
        } => {
            let plist: Vec<String> = params
                .iter()
                .map(|p| match &p.type_ann {
                    Some(t) => format!("{}: {t}", p.name),
                    None => p.name.clone(),
                })
                .collect();
            let ret = return_type.clone().unwrap_or_else(|| "void".into());
            let tps = if type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", type_params.join(", "))
            };
            println!("{pad}FnDecl {name}{tps}({}): {ret}", plist.join(", "));
            for s in body {
                print_stmt(ast, s, indent + 1);
            }
        }
        Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } => {
            let parts: Vec<String> = fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            let tps = if type_params.is_empty() {
                String::new()
            } else {
                format!("<{}>", type_params.join(", "))
            };
            println!("{pad}TypeDecl {name}{tps} = {{ {} }}", parts.join(", "));
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
        } => {
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
                let plist: Vec<String> = c
                    .params
                    .iter()
                    .map(|p| match &p.type_ann {
                        Some(t) => format!("{}: {t}", p.name),
                        None => p.name.clone(),
                    })
                    .collect();
                println!("{pad}  constructor({})", plist.join(", "));
                for s in &c.body {
                    print_stmt(ast, s, indent + 2);
                }
            }
            for m in methods {
                let plist: Vec<String> = m
                    .params
                    .iter()
                    .map(|p| match &p.type_ann {
                        Some(t) => format!("{}: {t}", p.name),
                        None => p.name.clone(),
                    })
                    .collect();
                let ret = m.return_type.clone().unwrap_or_else(|| "void".into());
                println!("{pad}  method {}({}): {ret}", m.name, plist.join(", "));
                for s in &m.body {
                    print_stmt(ast, s, indent + 2);
                }
            }
        }
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

pub(crate) fn print_expr(ast: &Ast, id: ExprId, indent: usize) {
    let pad = "  ".repeat(indent);
    match ast.get_expr(id) {
        Expr::Ident(n) => println!("{pad}Ident({n:?})"),
        Expr::String(s) => println!("{pad}String({s:?})"),
        Expr::Number(n) => println!("{pad}Number({n})"),
        Expr::BigInt { digits, radix } => println!("{pad}BigInt({digits}n, radix={radix})"),
        Expr::Bool(b) => println!("{pad}Bool({b})"),
        Expr::Null => println!("{pad}Null"),
        Expr::Uninit => println!("{pad}Uninit"),
        Expr::Regex { pattern, flags } => {
            println!("{pad}Regex /{pattern}/{flags}")
        }
        Expr::BinOp { op, left, right } => {
            println!("{pad}BinOp({op:?})");
            print_expr(ast, *left, indent + 1);
            print_expr(ast, *right, indent + 1);
        }
        Expr::Unary { op, expr } => {
            println!("{pad}Unary({op:?})");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::Member { obj, name } => {
            println!("{pad}Member");
            print_expr(ast, *obj, indent + 1);
            println!("{pad}  .{name}");
        }
        Expr::Call { callee, args } => {
            println!("{pad}Call");
            print_expr(ast, *callee, indent + 1);
            println!("{pad}  args:");
            for a in args {
                print_expr(ast, *a, indent + 2);
            }
        }
        Expr::Assign { target, value } => {
            println!("{pad}Assign");
            print_expr(ast, *target, indent + 1);
            println!("{pad}  =");
            print_expr(ast, *value, indent + 1);
        }
        Expr::Index { obj, index } => {
            println!("{pad}Index");
            print_expr(ast, *obj, indent + 1);
            println!("{pad}  [");
            print_expr(ast, *index, indent + 1);
            println!("{pad}  ]");
        }
        Expr::Array(elements) => {
            println!("{pad}Array [{}]", elements.len());
            for e in elements {
                print_expr(ast, *e, indent + 1);
            }
        }
        Expr::ObjectLit { fields } => {
            println!("{pad}ObjectLit {{");
            for (n, eid) in fields {
                println!("{pad}  {n}:");
                print_expr(ast, *eid, indent + 2);
            }
            println!("{pad}}}");
        }
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => {
            let plist: Vec<String> = params
                .iter()
                .map(|p| match &p.type_ann {
                    Some(t) => format!("{}: {t}", p.name),
                    None => p.name.clone(),
                })
                .collect();
            let ret = return_type.clone().unwrap_or_else(|| "void".into());
            println!("{pad}ArrowFn ({}) -> {ret}", plist.join(", "));
            for s in body {
                print_stmt(ast, s, indent + 1);
            }
        }
        Expr::Closure { fn_name, captures } => {
            println!("{pad}Closure {fn_name} captures=[{}]", captures.join(", "));
        }
        Expr::This => println!("{pad}This"),
        Expr::NewTarget => println!("{pad}NewTarget"),
        Expr::New { class_name, args } => {
            println!("{pad}New {class_name}");
            for a in args {
                print_expr(ast, *a, indent + 1);
            }
        }
        Expr::Super { args } => {
            println!("{pad}Super");
            for a in args {
                print_expr(ast, *a, indent + 1);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            println!("{pad}Ternary");
            print_expr(ast, *cond, indent + 1);
            print_expr(ast, *then_branch, indent + 1);
            print_expr(ast, *else_branch, indent + 1);
        }
        Expr::TypeOf { expr } => {
            println!("{pad}TypeOf");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::InstanceOf { expr, class_name } => {
            println!("{pad}InstanceOf {class_name}");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::Spread { expr } => {
            println!("{pad}Spread");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::Nullish { lhs, rhs } => {
            println!("{pad}Nullish");
            print_expr(ast, *lhs, indent + 1);
            print_expr(ast, *rhs, indent + 1);
        }
        Expr::OptChain { obj, name } => {
            println!("{pad}OptChain .{name}");
            print_expr(ast, *obj, indent + 1);
        }
        Expr::PostIncr { target, is_inc } => {
            println!("{pad}PostIncr is_inc={is_inc}");
            print_expr(ast, *target, indent + 1);
        }
        Expr::As { expr, ty_ann } => {
            println!("{pad}As {ty_ann}");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::Sequence { left, right } => {
            println!("{pad}Sequence");
            print_expr(ast, *left, indent + 1);
            print_expr(ast, *right, indent + 1);
        }
    }
}
