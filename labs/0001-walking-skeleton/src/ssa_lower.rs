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
// is wired into a full `tr build` driver.

use std::collections::HashMap;

use crate::ast::{self, Ast, BinOp as AstBinOp, Expr, ExprId, Stmt};
use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FPred, FuncId, IPred, InstKind, Module, Operand, Terminator,
    Type, ValueId,
};

pub fn lower(ast: &Ast) -> Module {
    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    // Pass 0: declare runtime intrinsics that the backend will implement.
    //   print_i64                — integer console.log fast-path
    //   __torajs_str_alloc       — copy `len` bytes from `src` into a fresh
    //                              heap StrRepr `{u64 len; u8 data[]}`.
    //                              Used for every string literal ever lowered.
    //   __torajs_str_print       — write StrRepr's bytes + trailing newline
    //                              to stdout. Replaces the old NUL-terminated
    //                              `print_str` (deleted in P2.2.b).
    let print_i64_id = declare_intrinsic(&mut module, &mut fn_table, "print_i64", &[Type::I64], Type::Void);
    let str_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_alloc",
        &[Type::Ptr, Type::I64],
        Type::Str,
    );
    let str_print_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_print",
        &[Type::Str],
        Type::Void,
    );
    let str_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_drop",
        &[Type::Str],
        Type::Void,
    );
    // `__torajs_str_concat(a, b) -> StrRepr*` — consumes both operands
    // (frees their backing heap), returns a freshly allocated StrRepr
    // holding `a.bytes ++ b.bytes`. ssa_lower routes `Expr::BinOp(Add,
    // str, str)` here.
    let str_concat_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_concat",
        &[Type::Str, Type::Str],
        Type::Str,
    );

    // Pass 1: pre-allocate FuncIds + record correct return types for every
    // user FnDecl. The placeholder body is empty; pass 2 fills it in. Setting
    // the right ret type up front lets callsites resolve `f_ret_type_hint`
    // even before the callee's body has been lowered (mutual recursion,
    // forward refs, return-type-bool functions like is_prime).
    let mut decl_indices: Vec<(usize, FuncId)> = Vec::new();
    for (i, stmt) in ast.stmts.iter().enumerate() {
        if let Stmt::FnDecl {
            name, return_type, ..
        } = stmt
        {
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(name.clone(), fid);
            module.funcs.push(ssa::Function::new(
                name.clone(),
                parse_type(return_type.as_deref()),
            ));
            decl_indices.push((i, fid));
        }
    }

    // Snapshot every callable's return type — used inside lower_fn to type
    // call-site results correctly.
    let signatures: HashMap<FuncId, Type> = module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (FuncId(i as u32), f.ret))
        .collect();

    let intrinsics = Intrinsics {
        print_i64: print_i64_id,
        str_alloc: str_alloc_id,
        str_print: str_print_id,
        str_drop: str_drop_id,
        str_concat: str_concat_id,
    };

    // Pass 2: lower user FnDecl bodies. Each call returns the lowered
    // function plus any string literals interned during its body; we
    // append those into module.strings before the next call so the
    // StringId counter stays in lockstep with module.strings.len().
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
        } = &ast.stmts[stmt_idx]
        {
            let (f, new_strings) = lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                &fn_table,
                &signatures,
                &intrinsics,
                module.strings.len(),
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
        }
    }

    // Pass 3: synthesize `main` from top-level non-FnDecl statements.
    let top_level: Vec<&Stmt> = ast
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::FnDecl { .. }))
        .collect();
    if !top_level.is_empty() {
        let (main_fn, new_strings) = synthesize_main(
            &top_level,
            ast,
            &fn_table,
            &signatures,
            &intrinsics,
            module.strings.len(),
        );
        for s in new_strings {
            module.strings.push(s);
        }
        module.funcs.push(main_fn);
    }

    module
}

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
struct Intrinsics {
    print_i64: FuncId,
    str_alloc: FuncId,
    str_print: FuncId,
    str_drop: FuncId,
    str_concat: FuncId,
}

#[derive(Debug, Clone, Copy)]
struct LocalInfo {
    /// Pointer to the alloca slot — Type::Ptr.
    slot: ValueId,
    /// Type of the slot's *contents* (what Load returns).
    ty: Type,
    /// True after the binding's value has been consumed. Drop emission at
    /// fn-end skips moved locals.
    moved: bool,
}

fn declare_intrinsic(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    name: &str,
    param_tys: &[Type],
    ret_ty: Type,
) -> FuncId {
    let mut f = ssa::Function::new(name, ret_ty);
    for (i, &t) in param_tys.iter().enumerate() {
        f.add_param(t, &format!("a{i}"));
    }
    // No blocks → declaration only; backend supplies the body.
    let id = FuncId(module.funcs.len() as u32);
    fn_table.insert(name.to_string(), id);
    module.funcs.push(f);
    id
}

fn synthesize_main(
    stmts: &[&Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    intrinsics: &Intrinsics,
    string_id_base: usize,
) -> (ssa::Function, Vec<Vec<u8>>) {
    let mut f = ssa::Function::new("main", Type::I32);
    let entry = f.add_block();
    let mut new_strings: Vec<Vec<u8>> = Vec::new();
    {
        let mut ctx = LowerCtx {
            f: &mut f,
            ast,
            fn_table,
            signatures,
            intrinsics: *intrinsics,
            locals: HashMap::new(),
            cur_block: entry,
            new_strings: &mut new_strings,
            string_id_base,
        };
        for s in stmts {
            ctx.lower_top_stmt(s);
        }
        if ctx.cur_open() {
            ctx.emit_drops_for_owned_locals();
            let cb = ctx.cur_block;
            ctx.f
                .set_term(cb, Terminator::Ret(Some(Operand::ConstI32(0))));
        }
    }
    (f, new_strings)
}

fn parse_type(ann: Option<&str>) -> Type {
    match ann {
        // `number` defaults to i64 — best for the integer-heavy cases
        // (popcount/fib40/gcd1m). When a function actually needs floating-
        // point semantics, the user annotates with the explicit `f64` type
        // (Rust-shaped). The dialect lets you opt in; you don't pay the
        // f64 tax just because TS spells everything `number`.
        Some("number") | Some("i64") => Type::I64,
        Some("f64") => Type::F64,
        Some("boolean") => Type::Bool,
        Some("string") => Type::Str,
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
    signatures: &HashMap<FuncId, Type>,
    intrinsics: &Intrinsics,
    string_id_base: usize,
) -> (ssa::Function, Vec<Vec<u8>>) {
    let ret_ty = parse_type(return_type);
    let mut f = ssa::Function::new(name, ret_ty);

    // Capture param SSA values + types BEFORE creating the entry block; we'll
    // alloca-and-store each one inside entry below so the lowerer can treat
    // params and let-locals uniformly (both read via Load, both writable via
    // Store; params just happen to be initialized from the function's
    // SSA-arg values).
    let mut param_setup: Vec<(String, ValueId, Type)> = Vec::with_capacity(params.len());
    for p in params {
        let pty = parse_type(p.type_ann.as_deref());
        let pid = f.add_param(pty, &p.name);
        param_setup.push((p.name.clone(), pid, pty));
    }

    let entry = f.add_block();
    // User function bodies can intern string literals (any `Expr::String`
    // routes through intern_string_literal). The base offset has the
    // current global string count — caller appends new_strings to
    // module.strings after this returns, so StringIds stay unique.
    let mut new_strings: Vec<Vec<u8>> = Vec::new();
    let mut ctx = LowerCtx {
        f: &mut f,
        ast,
        fn_table,
        signatures,
        intrinsics: *intrinsics,
        locals: HashMap::new(),
        cur_block: entry,
        new_strings: &mut new_strings,
        string_id_base,
    };

    // Materialize each param as an alloca-backed local. mem2reg at -O1+
    // collapses these straight back to the SSA arg values, so there is no
    // perf cost; we still get fib40 at 150 ms.
    for (name, pid, ty) in param_setup {
        let slot = ctx.alloca(ty, Some(&name));
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(pid), Operand::Value(slot)),
        );
        ctx.locals.insert(
            name,
            LocalInfo {
                slot,
                ty,
                moved: false,
            },
        );
    }

    for s in body {
        ctx.lower_stmt(s);
    }
    // Function fall-through (no explicit return). Emit drops + an implicit
    // void/zero return — applies to any block still open at body end.
    if ctx.cur_open() {
        ctx.emit_drops_for_owned_locals();
        let cb = ctx.cur_block;
        match ctx.f.ret {
            Type::Void => ctx.f.set_term(cb, Terminator::Ret(None)),
            _ => ctx.f.set_term(cb, Terminator::Unreachable),
        }
    }

    (f, new_strings)
}

struct LowerCtx<'a> {
    f: &'a mut ssa::Function,
    ast: &'a Ast,
    fn_table: &'a HashMap<String, FuncId>,
    /// FuncId → return type, populated in pass 1 of `lower`. Lets call-site
    /// lowering pick the right SSA result type even when the callee hasn't
    /// been body-lowered yet (forward refs, mutual recursion, bool returns).
    signatures: &'a HashMap<FuncId, Type>,
    /// Resolved FuncIds for the runtime intrinsics. Read at every site that
    /// emits a runtime call — string-literal lowering needs `str_alloc`,
    /// `console.log` needs `print_i64` / `str_print`, etc.
    intrinsics: Intrinsics,
    /// name → (alloca-ptr value, contents type, moved flag). Every local —
    /// including the function's own parameters — sits behind an alloca.
    /// mem2reg lifts them to SSA values at -O1+.
    ///
    /// `moved` mirrors check.rs's affine pass: when a binding's value is
    /// consumed (let-rhs, assign-rhs, non-Copy call-arg, return), the
    /// flag flips to true and Drop emission at fn-end skips that local.
    /// Insertion order preserved (LinkedHashMap-style) so multi-Ret
    /// drops fire in deterministic order — using IndexMap-equivalent
    /// behavior would be cleaner but a plain HashMap is fine for the
    /// number of locals our cases have.
    locals: HashMap<String, LocalInfo>,
    cur_block: BlockId,
    /// New string literals encountered during this lowering pass (currently
    /// only main collects them). Caller appends these to the module's
    /// strings table; StringId offsets are pre-assigned via string_id_base.
    new_strings: &'a mut Vec<Vec<u8>>,
    string_id_base: usize,
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

    /// Top-level statement lowering inside the synthesized `main` function.
    /// `console.log(<expr>)` dispatches on the lowered operand's type:
    ///   - Type::Str → `call print_str(<ptr>)`
    ///   - Type::I64 / others → `call print_i64(<value>)`
    /// Same dispatch handles literal strings (`Expr::String`) and string
    /// bindings — the literal path interns through `lower_expr`'s general
    /// `Expr::String` arm and gets the same Type::Str operand.
    fn lower_top_stmt(&mut self, s: &Stmt) {
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && self.is_console_log_member(*callee)
            && args.len() == 1
        {
            // `console.log` is borrow-style: doesn't consume its arg, so we
            // don't mark the source binding moved. But for *temp* args
            // (literal strings, BinOp results, Call returns), the Type::Str
            // value is owned-by-nobody after the call — drop it on the
            // spot to keep the heap from leaking.
            let is_borrow = matches!(self.ast.get_expr(args[0]), Expr::Ident(_));
            let arg = self.lower_expr(args[0]);
            let is_str = self.operand_ty(&arg) == Type::Str;
            let target = if is_str {
                self.intrinsics.str_print
            } else {
                self.intrinsics.print_i64
            };
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![arg]));
            if is_str && !is_borrow {
                let drop_fid = self.intrinsics.str_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![arg]),
                );
            }
            return;
        }
        self.lower_stmt(s);
    }

    /// `console.log` recognized as an Ident("console") + Member.name == "log".
    fn is_console_log_member(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Member { obj, name } if name == "log" => {
                matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "console")
            }
            _ => false,
        }
    }

    /// Allocate a stack slot of `ty` in the current block. Returns the
    /// alloca's pointer ValueId. Used for `let`-decl locals + parameter
    /// home-slots (see lower_fn).
    fn alloca(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(self.cur_block, InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// If `eid` resolves to a non-Copy `Ident(name)` binding, mark that
    /// binding as moved. No-op for Copy types (number/bool/etc) and for
    /// non-Ident expressions (literals, BinOp results, Call results).
    /// Mirrors check.rs's affine consume pass.
    fn consume_if_ident(&mut self, eid: ExprId) {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.locals.get_mut(&name)
                && !info.ty.is_copy()
            {
                info.moved = true;
            }
        }
    }

    /// Emit Drop calls for every owned non-Copy local in the current block.
    /// Called immediately before terminators that exit the function (Ret,
    /// fall-through). Skips `moved` bindings — those have transferred
    /// ownership elsewhere and the receiver is responsible for the drop.
    fn emit_drops_for_owned_locals(&mut self) {
        // Snapshot to avoid borrowing self.locals while we emit instructions
        // (which need &mut self.f). Cheap: bench cases have <10 locals each.
        let to_drop: Vec<(ValueId, Type)> = self
            .locals
            .values()
            .filter(|info| !info.moved && !info.ty.is_copy())
            .map(|info| (info.slot, info.ty))
            .collect();
        let drop_fid = self.intrinsics.str_drop;
        for (slot, ty) in to_drop {
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(slot)),
                ty,
                None,
            );
            self.f.append_void(
                self.cur_block,
                InstKind::Call(drop_fid, vec![Operand::Value(val)]),
            );
        }
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
            Stmt::LetDecl {
                mutable: _,
                name,
                type_ann,
                init,
            } => {
                // Step 4.1: every let goes through alloca regardless of `mutable`.
                // const-correctness check is the type-checker's job (already done in
                // check.rs); the SSA layer doesn't care.
                let ty = parse_type(type_ann.as_deref());
                let init_val = self.lower_expr(*init);
                self.consume_if_ident(*init);
                let slot = self.alloca(ty, Some(name));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(init_val, Operand::Value(slot)),
                );
                self.locals.insert(
                    name.clone(),
                    LocalInfo {
                        slot,
                        ty,
                        moved: false,
                    },
                );
            }
            Stmt::While { cond, body } => {
                let header = self.f.add_block();
                let body_blk = self.f.add_block();
                let after = self.f.add_block();

                self.f.set_term(self.cur_block, Terminator::Br(header));

                self.cur_block = header;
                let c = self.lower_expr(*cond);
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk: body_blk,
                        else_blk: after,
                    },
                );

                self.cur_block = body_blk;
                self.lower_stmt(body);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(header));
                }

                self.cur_block = after;
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
                // Lower the return value (if any) FIRST, then mark the
                // returned binding as moved so end-of-fn drop skips it,
                // THEN emit drops for everything still owned, THEN set
                // the Ret terminator. The order matters: we want the drop
                // calls to land in the same block as the Ret, before it.
                let ret_operand = maybe.map(|eid| {
                    let v = self.lower_expr(eid);
                    self.consume_if_ident(eid);
                    v
                });
                self.emit_drops_for_owned_locals();
                let cb = self.cur_block;
                self.f.set_term(cb, Terminator::Ret(ret_operand));
            }
            Stmt::Expr(eid) => {
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                let _ = self.lower_expr(*eid);
            }
            other => panic!("ssa-lower: unsupported stmt: {other:?}"),
        }
    }

    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    fn intern_string_literal(&mut self, s: &str) -> ValueId {
        let bytes = s.as_bytes().to_vec();
        let len = bytes.len() as i64;
        let sid =
            ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(bytes);
        let static_ptr = self.f.append_inst(
            self.cur_block,
            InstKind::StringRef(sid),
            Type::Ptr,
            None,
        );
        let alloc = self.intrinsics.str_alloc;
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                alloc,
                vec![Operand::Value(static_ptr), Operand::ConstI64(len)],
            ),
            Type::Str,
            None,
        )
    }

    fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let e = self.ast.get_expr(eid);
        match e {
            // Number literals coerce to i64 — type inference lifts them to
            // f64 once we wire numeric-mode detection into the lowerer.
            Expr::Number(n) => {
                // Integer-valued literals stay as i64; only literals with a
                // genuine fractional part become f64. `1.0` collapses to i64
                // here — but that's fine: when used in an f64 context, the
                // BinOp lowering below promotes ConstI64 → ConstF64 by
                // rewriting the operand. No SItoFP instruction is needed
                // for constants.
                if n.fract() != 0.0 {
                    Operand::ConstF64(*n)
                } else {
                    Operand::ConstI64(*n as i64)
                }
            }
            Expr::Bool(b) => Operand::ConstBool(*b),
            Expr::String(s) => {
                let s = s.clone();
                Operand::Value(self.intern_string_literal(&s))
            }
            Expr::Ident(name) => {
                let info = match self.locals.get(name) {
                    Some(i) => *i,
                    None => panic!("ssa-lower: unknown ident `{name}`"),
                };
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(info.ty, Operand::Value(info.slot)),
                    info.ty,
                    None,
                );
                Operand::Value(v)
            }
            Expr::Assign { target, value } => {
                // Only `Ident` on the lhs is supported in step 4.1. Member /
                // index assignments need objects and arrays (not in scope).
                let name = match self.ast.get_expr(*target) {
                    Expr::Ident(n) => n.clone(),
                    other => panic!("ssa-lower: unsupported assign target: {other:?}"),
                };
                let info = match self.locals.get(&name) {
                    Some(i) => *i,
                    None => panic!("ssa-lower: assign to unknown ident `{name}`"),
                };
                // Drop the previous value held in the slot before overwriting
                // — for non-Copy types only. Skipping this would leak the
                // old heap allocation. (For freshly let-decl'd locals this
                // path doesn't trigger because the let-init went through
                // Store directly without re-assigning.)
                if !info.ty.is_copy() && !info.moved {
                    let old = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(info.ty, Operand::Value(info.slot)),
                        info.ty,
                        None,
                    );
                    let drop_fid = self.intrinsics.str_drop;
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(drop_fid, vec![Operand::Value(old)]),
                    );
                }
                let v = self.lower_expr(*value);
                self.consume_if_ident(*value);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(v, Operand::Value(info.slot)),
                );
                // After Assign, the slot owns a fresh value — flip moved
                // back to false so subsequent reads work and end-of-fn
                // drop fires.
                if let Some(info) = self.locals.get_mut(&name) {
                    info.moved = false;
                }
                v
            }
            Expr::BinOp { op, left, right } => {
                let a = self.lower_expr(*left);
                let b = self.lower_expr(*right);
                // String concat consumes both operands — `let z = a + b`
                // moves both. Mark moved so end-of-fn drops skip them
                // (the concat runtime frees their backing heap when it
                // builds the result).
                if matches!(*op, AstBinOp::Add)
                    && self.operand_ty(&a) == Type::Str
                    && self.operand_ty(&b) == Type::Str
                {
                    self.consume_if_ident(*left);
                    self.consume_if_ident(*right);
                }
                self.lower_binop(*op, a, b)
            }
            Expr::Call { callee, args } => {
                let target = self.resolve_callee(*callee);
                let argv: Vec<Operand> = args.iter().map(|a| self.lower_expr(*a)).collect();
                // Consume non-Copy ident args. Mirrors check.rs's pass; the
                // SSA layer needs the same flag so end-of-fn drops skip
                // moved bindings.
                for a in args {
                    self.consume_if_ident(*a);
                }
                let ret_ty = self.f_ret_type_hint(target);
                let v = self
                    .f
                    .append_inst(self.cur_block, InstKind::Call(target, argv), ret_ty, None);
                Operand::Value(v)
            }
            other => panic!("ssa-lower: unsupported expr: {other:?}"),
        }
    }

    /// Type of the value produced by an operand. For SSA-Value operands this
    /// is the function's value-table lookup; for constants it's implied by
    /// the constant flavor.
    fn operand_ty(&self, op: &Operand) -> Type {
        match op {
            Operand::Value(v) => self.f.value_type(*v),
            Operand::ConstI64(_) => Type::I64,
            Operand::ConstI32(_) => Type::I32,
            Operand::ConstF64(_) => Type::F64,
            Operand::ConstBool(_) => Type::Bool,
        }
    }

    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::SiToFp(op),
                        Type::F64,
                        None,
                    );
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to f64"),
        }
    }

    /// Type-aware BinOp lowering. Decision rule:
    ///   - `/` always produces f64. Both operands coerced to f64. (Use `>>`
    ///     or explicit conversion for integer division — see collatz.tora.ts
    ///     for the convention.)
    ///   - Otherwise: if either operand is f64, both coerced to f64 and
    ///     a float-flavored op is emitted (FAdd/FSub/FMul, FCmp).
    ///   - Bitwise ops + Mod stay integer-only; mixing them with f64 is a
    ///     type error (caught at lower-time, not tolerated).
    fn lower_binop(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        // String concat short-circuit. Routes `str + str` to the runtime
        // concat intrinsic, which takes ownership of both operands.
        if matches!(op, AstBinOp::Add)
            && self.operand_ty(&a) == Type::Str
            && self.operand_ty(&b) == Type::Str
        {
            let concat = self.intrinsics.str_concat;
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(concat, vec![a, b]),
                Type::Str,
                None,
            );
            return Operand::Value(v);
        }

        let force_float = matches!(op, AstBinOp::Div);
        let either_float =
            self.operand_ty(&a) == Type::F64 || self.operand_ty(&b) == Type::F64;
        let is_float = force_float || either_float;

        if is_float {
            // Bitwise + Mod don't have an f64 equivalent in our IR; reject
            // explicitly rather than silently casting.
            match op {
                AstBinOp::Mod
                | AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr => {
                    panic!("ssa-lower: bitwise/mod op `{op:?}` requires i64 operands")
                }
                _ => {}
            }
            let af = self.coerce_to_f64(a);
            let bf = self.coerce_to_f64(b);
            return match op {
                AstBinOp::Add => self.bin(SsaBinOp::FAdd, af, bf, Type::F64),
                AstBinOp::Sub => self.bin(SsaBinOp::FSub, af, bf, Type::F64),
                AstBinOp::Mul => self.bin(SsaBinOp::FMul, af, bf, Type::F64),
                AstBinOp::Div => self.bin(SsaBinOp::FDiv, af, bf, Type::F64),
                AstBinOp::Lt => self.fcmp(FPred::Olt, af, bf),
                AstBinOp::Gt => self.fcmp(FPred::Ogt, af, bf),
                AstBinOp::Le => self.fcmp(FPred::Ole, af, bf),
                AstBinOp::Ge => self.fcmp(FPred::Oge, af, bf),
                AstBinOp::Eq => self.fcmp(FPred::Oeq, af, bf),
                AstBinOp::Neq => self.fcmp(FPred::One, af, bf),
                AstBinOp::Mod
                | AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr => unreachable!(),
            };
        }

        // i64 path (unchanged from step 4.1).
        match op {
            AstBinOp::Add => self.bin(SsaBinOp::Add, a, b, Type::I64),
            AstBinOp::Sub => self.bin(SsaBinOp::Sub, a, b, Type::I64),
            AstBinOp::Mul => self.bin(SsaBinOp::Mul, a, b, Type::I64),
            AstBinOp::Div => unreachable!("Div forced into float path above"),
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

    fn fcmp(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
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

    /// Look up the callee's return type from the signatures map populated
    /// in pass 1 of `lower`. Defaults to I64 for unknown FuncIds (intrinsics
    /// or forward refs we haven't catalogued yet — print_i64 returns void
    /// and is called via `append_void`, so its callsites never reach here).
    fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }
}
