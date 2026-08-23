//! The rest-param derived default ctor a ctor-less exotic subclass
//! gets when its builtin parent is NOT argument-count agnostic —
//! split from `desugar_classes_builtin_heritage` (the strip pass
//! answers "which parent, which kernels"; this file answers "what
//! the synthesized ctor body looks like").

use super::*;

/// The rest-param derived default ctor for a ctor-less exotic
/// subclass whose builtin parent is NOT argument-count agnostic:
///
/// ```text
/// constructor(...__superargs: any[]) {
///   if (__superargs.length === 0) { super(); }
///   else { super(__superargs[0]); }          // wrappers / Promise
///   // Array / RegExp / Date split the else again: 1 →
///   // super(args[0]), 2+ → the multi-argument kernel (Array hands
///   // the packed rest array to the §23.1.1.3 elements kernel,
///   // Date to the §21.4.2.1 components kernel, RegExp its first
///   // two slots to the §22.2.4.1 flags kernel).
/// }
/// ```
///
/// The wrappers and Promise ignore arguments past the first per
/// spec (§21.1.1.1 / §22.1.1.1 / §20.3.1.1 / §27.2.3.1 read only
/// the first), so the two-arm dispatch is semantically complete for
/// them. The `super` sites synthesized here are rewritten by the
/// caller's rewrite loop exactly like a user-written ctor's.
pub(super) fn synthesize_exotic_rest_ctor(ast: &mut Ast, parent: &str) -> crate::ast::ClassCtor {
    let args_len = |ast: &mut Ast| {
        let obj = ast.add_expr(Expr::Ident("__superargs".to_string()));
        ast.add_expr(Expr::Member {
            obj,
            name: "length".to_string(),
        })
    };
    let len0 = args_len(ast);
    let zero = ast.add_expr(Expr::Number(0.0));
    let is_zero = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: len0,
        right: zero,
    });
    let super0 = ast.add_expr(Expr::Super { args: Vec::new() });
    let arg0_obj = ast.add_expr(Expr::Ident("__superargs".to_string()));
    let idx0 = ast.add_expr(Expr::Number(0.0));
    let arg0 = ast.add_expr(Expr::Index {
        obj: arg0_obj,
        index: idx0,
    });
    let super1 = ast.add_expr(Expr::Super { args: vec![arg0] });
    let else_branch = if matches!(parent, "Array" | "RegExp" | "Date") {
        let len1 = args_len(ast);
        let one = ast.add_expr(Expr::Number(1.0));
        let is_one = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: len1,
            right: one,
        });
        let multi_arm = if matches!(parent, "Array" | "Date") {
            // Array §23.1.1.3 — the packed rest array IS the elements
            // list; the kernel appends each onto the minted cell.
            // Date §21.4.2.1 step 6 — the rest array carries the
            // components; the kernel runs ToNumber per present slot
            // (day defaults 1, time components 0) and writes the
            // clipped LOCAL-time ms into the mint. Both bypass the
            // super rewrite (already the lowered spelling — the
            // exotic-subclass magic dispatch owns them).
            let kernel = if parent == "Array" {
                "__torajs_arr_subclass_super_elems"
            } else {
                "__torajs_date_subclass_super_components"
            };
            let callee = ast.add_expr(Expr::Ident(kernel.to_string()));
            let this_id = ast.add_expr(Expr::This);
            let rest_id = ast.add_expr(Expr::Ident("__superargs".to_string()));
            Stmt::Expr(ast.add_expr(Expr::Call {
                callee,
                args: vec![this_id, rest_id],
            }))
        } else {
            // RegExp §22.2.4.1 — `(pattern, flags)` out of the rest
            // array's first two slots; extras were evaluated into the
            // array already and are ignored per ordinary-call
            // semantics. The outer dispatch admits only Array /
            // RegExp / Date here, so this arm IS the RegExp arm
            // (previously the loud not-yet-supported throw — RFC
            // 20260815 residue close, the family's last loud arm).
            let callee = ast.add_expr(Expr::Ident(
                "__torajs_regex_subclass_super_flags".to_string(),
            ));
            let this_id = ast.add_expr(Expr::This);
            let slot = |ast: &mut Ast, i: f64| {
                let obj = ast.add_expr(Expr::Ident("__superargs".to_string()));
                let idx = ast.add_expr(Expr::Number(i));
                ast.add_expr(Expr::Index { obj, index: idx })
            };
            let a0 = slot(ast, 0.0);
            let a1 = slot(ast, 1.0);
            Stmt::Expr(ast.add_expr(Expr::Call {
                callee,
                args: vec![this_id, a0, a1],
            }))
        };
        Stmt::If {
            cond: is_one,
            then_branch: Box::new(Stmt::Expr(super1)),
            else_branch: Some(Box::new(multi_arm)),
        }
    } else {
        Stmt::Expr(super1)
    };
    crate::ast::ClassCtor {
        params: vec![crate::ast::Param {
            name: "__superargs".to_string(),
            type_ann: Some("any[]".to_string()),
            default: None,
            is_rest: true,
        }],
        body: vec![Stmt::If {
            cond: is_zero,
            then_branch: Box::new(Stmt::Expr(super0)),
            else_branch: Some(Box::new(else_branch)),
        }],
    }
}

/// The buffer-family derived default ctor — §23.2.5.1 reads at most
/// three arguments and is argument-count agnostic (ToIndex maps
/// undefined to 0, and an undefined explicit length means "to the
/// buffer's end"), so a fixed three-slot forward is EXACT: a missing
/// call argument arrives as the same undefined a missing super slot
/// would.
pub(super) fn synthesize_typedarray_forward_ctor(ast: &mut Ast) -> crate::ast::ClassCtor {
    let args: Vec<ExprId> = ["__sa0", "__sa1", "__sa2"]
        .iter()
        .map(|n| ast.add_expr(Expr::Ident(n.to_string())))
        .collect();
    let sup = ast.add_expr(Expr::Super { args });
    crate::ast::ClassCtor {
        params: ["__sa0", "__sa1", "__sa2"]
            .iter()
            .map(|n| crate::ast::Param {
                name: n.to_string(),
                type_ann: Some("any".to_string()),
                default: None,
                is_rest: false,
            })
            .collect(),
        body: vec![Stmt::Expr(sup)],
    }
}
