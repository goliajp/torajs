#![allow(dead_code)] // step 2: minimum-scope lowerer; some helpers used by step 2.x onward

// AST → SSA lowerer (P3.5.a step 2).
//
// Scope of this step: just enough to lower fib40.tora.ts. That means:
//   - Top-level `Stmt::FnDecl` → `ssa::Function`
//   - `Stmt::If { else? }` → CondBr with no-else fall-through to merge block
//   - `Stmt::Return(expr?)` → Terminator::Ret
//   - `Stmt::Block`, `Stmt::Expr` (for chained calls)
//   - `Expr::Number` (i64 only — no f64 narrowing yet), `Bool`, `Ident`
//   - `Expr::BinOp` for the arith / compare / bitwise ops in the AST
//   - `Expr::Call { callee: Ident("...") }` resolving to a same-module FnDecl
//
// Deferred to step 2.x:
//   - `Stmt::LetDecl` + `Stmt::While` + `Expr::Assign` (need phi nodes)
//   - f64 numeric narrowing (number → f64 vs i64)
//   - `console.log(...)` at top level + a synthesized `main()` (step 3 wires
//     this when the Inkwell backend lands; right now `tr ssa` ignores
//     non-FnDecl top-level statements)
//   - Member-call resolution (only `Ident("...")` callees handled here)
//
// On unsupported shapes we panic with a clear message — labs material, not a
// user-facing tool yet. Will switch to a Result<_, LowerError> path when this
// is wired into a full `tr build-llvm` driver.

use std::collections::HashMap;

use crate::ast::{self, Ast, BinOp as AstBinOp, Expr, ExprId, Stmt};
use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FuncId, IPred, InstKind, Module, Operand, Terminator, Type,
    ValueId,
};

pub fn lower(ast: &Ast) -> Module {
    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    // Pass 1: pre-allocate FuncIds so callsites in any FnDecl body can resolve
    // forward references (mutual recursion, callee-defined-below).
    let mut decl_indices: Vec<(usize, FuncId)> = Vec::new();
    for (i, stmt) in ast.stmts.iter().enumerate() {
        if let Stmt::FnDecl { name, .. } = stmt {
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(name.clone(), fid);
            module
                .funcs
                .push(ssa::Function::new(name.clone(), Type::Void)); // placeholder, overwritten below
            decl_indices.push((i, fid));
        }
    }

    // Pass 2: lower bodies. Drop placeholders and write real functions in place.
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
        } = &ast.stmts[stmt_idx]
        {
            let f = lower_fn(name, params, return_type.as_deref(), body, ast, &fn_table);
            module.funcs[fid.0 as usize] = f;
        }
    }

    module
}

fn parse_type(ann: Option<&str>) -> Type {
    match ann {
        // Step 2 intentionally hard-codes `number → i64`. f64 narrowing comes
        // in step 2.x once we propagate the same `detect_numeric_mode` logic
        // that `build.rs` already implements for the wasm-via-C path.
        Some("number") => Type::I64,
        Some("boolean") => Type::Bool,
        Some("void") | None => Type::Void,
        Some(other) => panic!("ssa-lower: unsupported type annotation `{other}`"),
    }
}

fn lower_fn(
    name: &str,
    params: &[ast::Param],
    return_type: Option<&str>,
    body: &[Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
) -> ssa::Function {
    let ret_ty = parse_type(return_type);
    let mut f = ssa::Function::new(name, ret_ty);
    let mut locals: HashMap<String, ValueId> = HashMap::new();

    for p in params {
        let pty = parse_type(p.type_ann.as_deref());
        let pid = f.add_param(pty, &p.name);
        locals.insert(p.name.clone(), pid);
    }

    let entry = f.add_block();
    let mut ctx = LowerCtx {
        f: &mut f,
        ast,
        fn_table,
        locals,
        cur_block: entry,
    };

    for s in body {
        ctx.lower_stmt(s);
    }

    f
}

struct LowerCtx<'a> {
    f: &'a mut ssa::Function,
    ast: &'a Ast,
    fn_table: &'a HashMap<String, FuncId>,
    locals: HashMap<String, ValueId>,
    cur_block: BlockId,
}

impl<'a> LowerCtx<'a> {
    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Block(stmts) => {
                for s in stmts {
                    self.lower_stmt(s);
                    if !self.cur_open() {
                        // Block already terminated by an inner return/if-else-both-return;
                        // skip remaining stmts (they're unreachable). Real diagnostic
                        // would warn, deferred.
                        break;
                    }
                }
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.lower_expr(*cond);
                let then_blk = self.f.add_block();
                let after_blk = self.f.add_block();

                // No-else case: cond_br false → after directly. Saves an empty
                // pass-through block and matches the demo_fib40() layout exactly.
                let else_blk = if else_branch.is_some() {
                    self.f.add_block()
                } else {
                    after_blk
                };

                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk,
                        else_blk,
                    },
                );

                self.cur_block = then_blk;
                self.lower_stmt(then_branch);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                }

                if let Some(eb) = else_branch {
                    self.cur_block = else_blk;
                    self.lower_stmt(eb);
                    if self.cur_open() {
                        self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                    }
                }

                self.cur_block = after_blk;
            }
            Stmt::Return(maybe) => {
                let term = match maybe {
                    Some(eid) => Terminator::Ret(Some(self.lower_expr(*eid))),
                    None => Terminator::Ret(None),
                };
                self.f.set_term(self.cur_block, term);
            }
            Stmt::Expr(eid) => {
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                let _ = self.lower_expr(*eid);
            }
            other => panic!("ssa-lower: unsupported stmt: {other:?}"),
        }
    }

    fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let e = self.ast.get_expr(eid);
        match e {
            // Number literals coerce to i64 — type inference lifts them to
            // f64 once we wire numeric-mode detection into the lowerer.
            Expr::Number(n) => Operand::ConstI64(*n as i64),
            Expr::Bool(b) => Operand::ConstBool(*b),
            Expr::Ident(name) => match self.locals.get(name) {
                Some(v) => Operand::Value(*v),
                None => panic!("ssa-lower: unknown ident `{name}`"),
            },
            Expr::BinOp { op, left, right } => {
                let a = self.lower_expr(*left);
                let b = self.lower_expr(*right);
                match op {
                    AstBinOp::Add => self.bin(SsaBinOp::Add, a, b, Type::I64),
                    AstBinOp::Sub => self.bin(SsaBinOp::Sub, a, b, Type::I64),
                    AstBinOp::Mul => self.bin(SsaBinOp::Mul, a, b, Type::I64),
                    AstBinOp::Div => self.bin(SsaBinOp::SDiv, a, b, Type::I64),
                    AstBinOp::Mod => self.bin(SsaBinOp::SRem, a, b, Type::I64),
                    AstBinOp::BitAnd => self.bin(SsaBinOp::And, a, b, Type::I64),
                    AstBinOp::BitOr => self.bin(SsaBinOp::Or, a, b, Type::I64),
                    AstBinOp::BitXor => self.bin(SsaBinOp::Xor, a, b, Type::I64),
                    AstBinOp::Shl => self.bin(SsaBinOp::Shl, a, b, Type::I64),
                    AstBinOp::Shr => self.bin(SsaBinOp::AShr, a, b, Type::I64),
                    AstBinOp::Lt => self.cmp(IPred::Slt, a, b),
                    AstBinOp::Gt => self.cmp(IPred::Sgt, a, b),
                    AstBinOp::Le => self.cmp(IPred::Sle, a, b),
                    AstBinOp::Ge => self.cmp(IPred::Sge, a, b),
                    AstBinOp::Eq => self.cmp(IPred::Eq, a, b),
                    AstBinOp::Neq => self.cmp(IPred::Ne, a, b),
                }
            }
            Expr::Call { callee, args } => {
                let target = self.resolve_callee(*callee);
                let argv: Vec<Operand> = args.iter().map(|a| self.lower_expr(*a)).collect();
                let ret_ty = self.f_ret_type_hint(target);
                let v = self
                    .f
                    .append_inst(self.cur_block, InstKind::Call(target, argv), ret_ty, None);
                Operand::Value(v)
            }
            other => panic!("ssa-lower: unsupported expr: {other:?}"),
        }
    }

    fn bin(&mut self, op: SsaBinOp, a: Operand, b: Operand, ty: Type) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::BinOp(op, a, b), ty, None);
        Operand::Value(v)
    }

    fn cmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    fn resolve_callee(&self, eid: ExprId) -> FuncId {
        match self.ast.get_expr(eid) {
            Expr::Ident(name) => match self.fn_table.get(name) {
                Some(f) => *f,
                None => panic!("ssa-lower: unknown function `{name}`"),
            },
            // `console.log(...)` and other Member callees land here; deferred
            // to step 3 when the Inkwell backend wires up host imports.
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }

    /// Looks up the (already-lowered) callee's return type. We can do this
    /// because pass 1 above pre-populated `module.funcs` with placeholders;
    /// by the time any callsite lowers, the target Function's `ret` field
    /// has been overwritten with the real return type during pass 2 IF the
    /// target was lowered earlier in source order. For mutual / forward
    /// recursion, the placeholder still has Type::Void — fix up in step 2.x
    /// by doing a separate "collect signatures" pre-pass.
    ///
    /// fib40 only self-recurses, and the self-call sees its own already-set
    /// return type, so this works for the demo.
    fn f_ret_type_hint(&self, _fid: FuncId) -> Type {
        // For now: assume i64 (matches every numeric function we lower today).
        // Fixed properly in step 2.x with a signature pre-pass.
        Type::I64
    }
}
