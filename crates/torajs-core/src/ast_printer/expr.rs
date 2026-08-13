//! `print_expr` half of the AST pretty-printer — split out of
//! `ast_printer.rs` (chunk 454) to keep both halves under the
//! 500-line file limit. Mutually recursive with `print_stmt`;
//! the shared `fmt_params` helper lives in the parent module.

use crate::ast::{Ast, Expr, ExprId};

use super::{fmt_params, print_stmt};

pub(crate) fn print_expr(ast: &Ast, id: ExprId, indent: usize) {
    let pad = "  ".repeat(indent);
    match ast.get_expr(id) {
        Expr::Elision => println!("{pad}Elision"),
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
            let ret = return_type.clone().unwrap_or_else(|| "void".into());
            println!("{pad}ArrowFn ({}) -> {ret}", fmt_params(params));
            for s in body {
                print_stmt(ast, s, indent + 1);
            }
        }
        Expr::Closure { fn_name, captures } => {
            println!("{pad}Closure {fn_name} captures=[{}]", captures.join(", "));
        }
        Expr::This => println!("{pad}This"),
        Expr::NewTarget => println!("{pad}NewTarget"),
        Expr::New {
            class_name, args, ..
        } => {
            println!("{pad}New {class_name}");
            for a in args {
                print_expr(ast, *a, indent + 1);
            }
        }
        Expr::NewDynamic { callee, args } => {
            println!("{pad}NewDynamic");
            print_expr(ast, *callee, indent + 1);
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
        Expr::Delete { expr } => {
            println!("{pad}Delete");
            print_expr(ast, *expr, indent + 1);
        }
        Expr::InstanceOf { expr, rhs } => {
            println!("{pad}InstanceOf");
            print_expr(ast, *expr, indent + 1);
            print_expr(ast, *rhs, indent + 1);
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
        Expr::OptIndex { obj, index } => {
            println!("{pad}OptIndex");
            print_expr(ast, *obj, indent + 1);
            print_expr(ast, *index, indent + 1);
        }
        Expr::OptCall { callee, args } => {
            println!("{pad}OptCall");
            print_expr(ast, *callee, indent + 1);
            println!("{pad}  args:");
            for a in args {
                print_expr(ast, *a, indent + 2);
            }
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
