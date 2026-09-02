//! `Formatter::fmt_expr` — `tr fmt` per-Expr emission. Separate impl
//! block of `Formatter` (Rust allows multiple impl blocks on the
//! same type within a crate). Plus the `binop_str` operator
//! stringifier helper.
//!
//! Extracted from `formatter.rs` (2026-05-25, god-file decomp batch 17).

use crate::ast::PropKey;
use crate::ast::{BinOp, Expr, ExprId, Stmt, UnaryOp};

use super::Formatter;

impl<'a> Formatter<'a> {
    pub(super) fn fmt_expr(&mut self, eid: ExprId) {
        let e = self.ast.get_expr(eid);
        match e {
            // An elision prints as its bare slot — the Array arm's
            // comma-join renders `[0, , 2]`.
            Expr::Elision => {}
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
            Expr::String(s) => self.fmt_string_lit(s),
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
            Expr::OptIndex { obj, index } => {
                self.fmt_expr(*obj);
                self.write("?.[");
                self.fmt_expr(*index);
                self.write("]");
            }
            Expr::OptCall { callee, args } => {
                self.fmt_expr(*callee);
                self.write("?.(");
                self.fmt_comma_list(args);
                self.write(")");
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
                self.fmt_comma_list(args);
                self.write(")");
            }
            Expr::Assign { target, value } => {
                self.fmt_expr(*target);
                self.write(" = ");
                self.fmt_expr(*value);
            }
            Expr::Array(items) => {
                self.write("[");
                self.fmt_comma_list(items);
                self.write("]");
            }
            Expr::Spread { expr } => {
                self.write("...");
                self.fmt_expr(*expr);
            }
            Expr::ObjectLit { fields } => self.fmt_object_lit(fields),
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
                    self.fmt_block_braces(body);
                }
            }
            Expr::Closure { fn_name, captures } => self.fmt_closure_hint(fn_name, captures),
            Expr::This => self.write("this"),
            Expr::New {
                class_name, args, ..
            } => {
                self.write("new ");
                self.write(class_name);
                self.write("(");
                self.fmt_comma_list(args);
                self.write(")");
            }
            Expr::NewDynamic { callee, args } => {
                self.write("new ");
                self.fmt_expr(*callee);
                self.write("(");
                self.fmt_comma_list(args);
                self.write(")");
            }
            Expr::Super { args } => {
                self.write("super(");
                self.fmt_comma_list(args);
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
            Expr::Delete { expr } => {
                self.write("delete ");
                self.fmt_expr(*expr);
            }
            Expr::InstanceOf { expr, rhs } => {
                self.fmt_expr(*expr);
                self.write(" instanceof ");
                self.fmt_expr(*rhs);
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
            Expr::Unary { op, expr } => self.fmt_unary(op, *expr),
            Expr::BinOp { op, left, right } => self.fmt_binop(op, *left, *right),
        }
    }

    /// Comma-separated expr list — the `(a, b, c)` / `[a, b, c]`
    /// interior shared by Call / Array / New / Super.
    fn fmt_comma_list(&mut self, items: &[ExprId]) {
        for (i, a) in items.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.fmt_expr(*a);
        }
    }

    /// Single-quoted string literal with escape folding.
    fn fmt_string_lit(&mut self, s: &torajs_wtf8::Wtf8) {
        self.write("'");
        for cp in s.code_points() {
            match char::from_u32(cp) {
                Some('\\') => self.write("\\\\"),
                Some('\'') => self.write("\\'"),
                Some('\n') => self.write("\\n"),
                Some('\t') => self.write("\\t"),
                Some('\r') => self.write("\\r"),
                Some(c) => self.out.push(c),
                // lone surrogate: only an escape can spell it
                None => self.write(&format!("\\u{cp:04x}")),
            }
        }
        self.write("'");
    }

    fn fmt_object_lit(&mut self, fields: &[(PropKey, ExprId)]) {
        self.write("{ ");
        for (i, (n, v)) in fields.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            // shorthand: `{ x }` if value is an ident with the same name
            if let Expr::Ident(vn) = self.ast.get_expr(*v)
                && n == vn
            {
                self.write(&n.to_string_lossy_owned());
            } else {
                self.write(&n.to_string_lossy_owned());
                self.write(": ");
                self.fmt_expr(*v);
            }
        }
        self.write(" }");
    }

    /// Synthetic shape — only appears post-`lift_arrow_fns`.
    /// Pre-desugar `format()` shouldn't see this. Print
    /// recognizable but unparseable hint so users notice.
    fn fmt_closure_hint(&mut self, fn_name: &str, captures: &[String]) {
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

    fn fmt_unary(&mut self, op: &UnaryOp, expr: ExprId) {
        let s = match op {
            UnaryOp::Not => "!",
            UnaryOp::Neg => "-",
            UnaryOp::BitNot => "~",
            UnaryOp::Plus => "+",
        };
        self.write(s);
        // Parenthesize complex operands defensively.
        let needs_paren = matches!(
            self.ast.get_expr(expr),
            Expr::BinOp { .. } | Expr::Ternary { .. } | Expr::Assign { .. }
        );
        if needs_paren {
            self.write("(");
        }
        self.fmt_expr(expr);
        if needs_paren {
            self.write(")");
        }
    }

    fn fmt_binop(&mut self, op: &BinOp, left: ExprId, right: ExprId) {
        let needs_paren = |child: ExprId| matches!(self.ast.get_expr(child), Expr::BinOp { .. });
        if needs_paren(left) {
            self.write("(");
            self.fmt_expr(left);
            self.write(")");
        } else {
            self.fmt_expr(left);
        }
        self.write(" ");
        self.write(binop_str(op));
        self.write(" ");
        if needs_paren(right) {
            self.write("(");
            self.fmt_expr(right);
            self.write(")");
        } else {
            self.fmt_expr(right);
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
