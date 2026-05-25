//! `Formatter::fmt_expr` — `tr fmt` per-Expr emission. Separate impl
//! block of `Formatter` (Rust allows multiple impl blocks on the
//! same type within a crate). Plus the `binop_str` operator
//! stringifier helper.
//!
//! Extracted from `formatter.rs` (2026-05-25, god-file decomp batch 17).

use crate::ast::{BinOp, Expr, ExprId, Stmt, UnaryOp};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn fmt_expr(&mut self, eid: ExprId) {
        let e = self.ast.get_expr(eid);
        match e {
            Expr::Ident(n) => self.write(n),
            Expr::NewTarget => self.write("new.target"),
            Expr::Number(n) => {
                // Prefer integer form when the f64 round-trips;
                // otherwise %g — mirrors `console.log` semantics.
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    self.write(&format!("{}", *n as i64));
                } else {
                    self.write(&format!("{n}"));
                }
            }
            Expr::BigInt { digits, radix } => {
                let prefix = match *radix {
                    16 => "0x",
                    2 => "0b",
                    8 => "0o",
                    _ => "",
                };
                self.write(&format!("{prefix}{digits}n"));
            }
            Expr::Bool(b) => self.write(if *b { "true" } else { "false" }),
            Expr::Null => self.write("null"),
            Expr::Uninit => {} // declared-but-uninit; handled by LetDecl
            Expr::String(s) => {
                self.write("'");
                for c in s.chars() {
                    match c {
                        '\\' => self.write("\\\\"),
                        '\'' => self.write("\\'"),
                        '\n' => self.write("\\n"),
                        '\t' => self.write("\\t"),
                        '\r' => self.write("\\r"),
                        c => self.out.push(c),
                    }
                }
                self.write("'");
            }
            Expr::Regex { pattern, flags } => {
                self.write("/");
                self.write(pattern);
                self.write("/");
                self.write(flags);
            }
            Expr::Member { obj, name } => {
                self.fmt_expr(*obj);
                self.write(".");
                self.write(name);
            }
            Expr::OptChain { obj, name } => {
                self.fmt_expr(*obj);
                self.write("?.");
                self.write(name);
            }
            Expr::Index { obj, index } => {
                self.fmt_expr(*obj);
                self.write("[");
                self.fmt_expr(*index);
                self.write("]");
            }
            Expr::Call { callee, args } => {
                self.fmt_expr(*callee);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Assign { target, value } => {
                self.fmt_expr(*target);
                self.write(" = ");
                self.fmt_expr(*value);
            }
            Expr::Array(items) => {
                self.write("[");
                for (i, a) in items.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write("]");
            }
            Expr::Spread { expr } => {
                self.write("...");
                self.fmt_expr(*expr);
            }
            Expr::ObjectLit { fields } => {
                self.write("{ ");
                for (i, (n, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    // shorthand: `{ x }` if value is an ident with the same name
                    if let Expr::Ident(vn) = self.ast.get_expr(*v)
                        && vn == n
                    {
                        self.write(n);
                    } else {
                        self.write(n);
                        self.write(": ");
                        self.fmt_expr(*v);
                    }
                }
                self.write(" }");
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                self.fmt_params(params);
                if let Some(ret) = return_type {
                    self.write(": ");
                    self.write(ret);
                }
                self.write(" => ");
                if body.len() == 1
                    && let Stmt::Return(Some(eid)) = &body[0]
                {
                    self.fmt_expr(*eid);
                } else {
                    self.write("{");
                    self.newline();
                    self.indent += 1;
                    for s in body {
                        self.fmt_stmt(s);
                        self.newline();
                    }
                    self.indent -= 1;
                    self.write_indent();
                    self.write("}");
                }
            }
            Expr::Closure { fn_name, captures } => {
                // Synthetic shape — only appears post-`lift_arrow_fns`.
                // Pre-desugar `format()` shouldn't see this. Print
                // recognizable but unparseable hint so users notice.
                self.write("/*closure ");
                self.write(fn_name);
                self.write(" captures=[");
                for (i, c) in captures.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.write(c);
                }
                self.write("]*/");
            }
            Expr::This => self.write("this"),
            Expr::New { class_name, args } => {
                self.write("new ");
                self.write(class_name);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Super { args } => {
                self.write("super(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.fmt_expr(*a);
                }
                self.write(")");
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.fmt_expr(*cond);
                self.write(" ? ");
                self.fmt_expr(*then_branch);
                self.write(" : ");
                self.fmt_expr(*else_branch);
            }
            Expr::TypeOf { expr } => {
                self.write("typeof ");
                self.fmt_expr(*expr);
            }
            Expr::InstanceOf { expr, class_name } => {
                self.fmt_expr(*expr);
                self.write(" instanceof ");
                self.write(class_name);
            }
            Expr::Nullish { lhs, rhs } => {
                self.fmt_expr(*lhs);
                self.write(" ?? ");
                self.fmt_expr(*rhs);
            }
            Expr::PostIncr { target, is_inc } => {
                self.fmt_expr(*target);
                self.write(if *is_inc { "++" } else { "--" });
            }
            Expr::As { expr, ty_ann } => {
                self.fmt_expr(*expr);
                self.write(" as ");
                self.write(ty_ann);
            }
            Expr::Sequence { left, right } => {
                self.write("(");
                self.fmt_expr(*left);
                self.write(", ");
                self.fmt_expr(*right);
                self.write(")");
            }
            Expr::Unary { op, expr } => {
                let s = match op {
                    UnaryOp::Not => "!",
                    UnaryOp::Neg => "-",
                    UnaryOp::BitNot => "~",
                    UnaryOp::Plus => "+",
                };
                self.write(s);
                // Parenthesize complex operands defensively.
                let needs_paren = matches!(
                    self.ast.get_expr(*expr),
                    Expr::BinOp { .. } | Expr::Ternary { .. } | Expr::Assign { .. }
                );
                if needs_paren {
                    self.write("(");
                }
                self.fmt_expr(*expr);
                if needs_paren {
                    self.write(")");
                }
            }
            Expr::BinOp { op, left, right } => {
                let needs_paren =
                    |child: ExprId| matches!(self.ast.get_expr(child), Expr::BinOp { .. });
                if needs_paren(*left) {
                    self.write("(");
                    self.fmt_expr(*left);
                    self.write(")");
                } else {
                    self.fmt_expr(*left);
                }
                self.write(" ");
                self.write(binop_str(op));
                self.write(" ");
                if needs_paren(*right) {
                    self.write("(");
                    self.fmt_expr(*right);
                    self.write(")");
                } else {
                    self.fmt_expr(*right);
                }
            }
        }
    }
}

fn binop_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::LAnd => "&&",
        BinOp::LOr => "||",
        BinOp::Eq => "===",
        BinOp::Neq => "!==",
        BinOp::LooseEq => "==",
        BinOp::LooseNeq => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
    }
}
