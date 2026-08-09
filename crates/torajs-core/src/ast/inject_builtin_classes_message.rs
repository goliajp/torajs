//! §20.5.1.1 step-3 `message` install for the injected Error root
//! ctor — sibling of `inject_builtin_classes` (the parent file sat at
//! the 500-line limit; the cause face lives in
//! `inject_builtin_classes_cause`, the data-param subclasses in
//! `inject_builtin_classes_data`, and the message-coercion builder
//! here).

use super::inject_builtin_classes::build_absent_sentinel;
use super::{Ast, BinOp, Expr, ExprId, Stmt};

/// §20.5.1.1 step 3 — the `this.message` install, single copy here in
/// the root ctor (subclasses forward through `super`). A three-arm
/// if/else STATEMENT, not a ternary:
///
/// ```ts
/// if (message === undefined) this.message = __torajs_undef_str();
/// else if (typeof message === "string") this.message = message;
/// else this.message = "" + message;
/// ```
///
/// - the `undefined` arm covers both an absent argument (the param's
///   default is plain `undefined` — see below) and an explicit one:
///   spec defines no own `message`, so the arm assigns the
///   own-absence sentinel as a DIRECT call-result store. The
///   sentinel's absence semantics ride its cell identity, and the
///   `message: any` param means anything routed through the
///   parameter is NaN-boxed — the owned unbox materializes ShortStrs
///   into fresh cells, which strips that identity. This is also why
///   the param default is `undefined` rather than the sentinel
///   itself (the pre-`any` layout could pass it through the typed
///   string slot untouched);
/// - the string arm stores the common case verbatim;
/// - the concat arm is the ToString, with spec fidelity a
///   `String(message)` call would lose — §22.1.1 String() answers a
///   SymbolDescriptiveString where ToString(Symbol) must throw a
///   TypeError, and `"" + sym` throws.
pub(super) fn build_message_install(ast: &mut Ast) -> Stmt {
    let assign_to_message = |ast: &mut Ast, value: ExprId| -> Stmt {
        let this = ast.add_expr(Expr::This);
        let member = ast.add_expr(Expr::Member {
            obj: this,
            name: "message".to_string(),
        });
        let assign = ast.add_expr(Expr::Assign {
            target: member,
            value,
        });
        Stmt::Expr(assign)
    };

    let msg_u = ast.add_expr(Expr::Ident("message".to_string()));
    let undef = ast.add_expr(Expr::Ident("undefined".to_string()));
    let is_undef = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: msg_u,
        right: undef,
    });
    let sentinel = build_absent_sentinel(ast);
    let absent_arm = assign_to_message(ast, sentinel);

    let msg0 = ast.add_expr(Expr::Ident("message".to_string()));
    let type_of = ast.add_expr(Expr::TypeOf { expr: msg0 });
    let str_lit = ast.add_expr(Expr::String("string".to_string()));
    let is_str = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: type_of,
        right: str_lit,
    });
    let msg_then = ast.add_expr(Expr::Ident("message".to_string()));
    let verbatim_arm = assign_to_message(ast, msg_then);

    let empty = ast.add_expr(Expr::String(String::new()));
    let msg_else = ast.add_expr(Expr::Ident("message".to_string()));
    let concat = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: empty,
        right: msg_else,
    });
    let coerce_arm = assign_to_message(ast, concat);

    let inner_if = Stmt::If {
        cond: is_str,
        then_branch: Box::new(verbatim_arm),
        else_branch: Some(Box::new(coerce_arm)),
    };
    Stmt::If {
        cond: is_undef,
        then_branch: Box::new(absent_arm),
        else_branch: Some(Box::new(inner_if)),
    }
}
