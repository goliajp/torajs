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
    let print_f64_id = declare_intrinsic(&mut module, &mut fn_table, "print_f64", &[Type::F64], Type::Void);
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
    // P2.4.c: object heap alloc + drop. Layout is the lowerer's call —
    // pass the byte size as i64. The runtime is just malloc/free.
    let obj_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_obj_alloc",
        &[Type::I64],
        Type::Ptr,
    );
    let obj_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_obj_drop",
        &[Type::Ptr],
        Type::Void,
    );
    // P2.3.b: Rc<T> runtime. Layout `{u64 strong, u64 weak, payload}` — 16
    // byte header, payload at offset 16. Rc.new lowers to alloc + N stores
    // at offset 16+i*8. Clone bumps strong_count and returns the same ptr.
    // Drop emission for Rc bindings is deferred to P2.3.c (where the
    // per-T drop_payload thunk lands); the rc_drop intrinsic is declared
    // now so the runtime symbol table is stable across P2.3.b → c.
    let rc_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_rc_alloc",
        &[Type::I64],
        Type::Ptr,
    );
    let rc_clone_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_rc_clone",
        &[Type::Ptr],
        Type::Ptr,
    );
    let rc_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_rc_drop",
        &[Type::Ptr],
        Type::Void,
    );
    // stdlib `Math` namespace — first slice. All take an f64 and return
    // an f64; the lowerer auto-promotes integer args via SiToFp at the
    // call site. Backed by libc sqrt / fabs / floor / ceil via thin
    // wrappers in each backend.
    let math_sqrt_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_sqrt",
        &[Type::F64],
        Type::F64,
    );
    let math_abs_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_abs",
        &[Type::F64],
        Type::F64,
    );
    let math_floor_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_floor",
        &[Type::F64],
        Type::F64,
    );
    let math_ceil_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_ceil",
        &[Type::F64],
        Type::F64,
    );
    let math_log_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_log",
        &[Type::F64],
        Type::F64,
    );
    let math_exp_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_exp",
        &[Type::F64],
        Type::F64,
    );
    let math_pow_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_pow",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_min_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_min",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_max_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_max",
        &[Type::F64, Type::F64],
        Type::F64,
    );

    // Pass 0.5: register user-declared type aliases. `type Point = { x:
    // number, y: number }` interns the layout in `module.struct_layouts`
    // and adds `Point → Type::Obj(StructId)` to `aliases`. Order matters:
    // forward references between aliases aren't supported (matches
    // check.rs's behavior — would error there before reaching here).
    //
    // `rc_layouts` lives outside `module` during lowering — it's a `&mut
    // Vec<Type>` threaded through every parse_type call site so any
    // `Rc<X>` annotation encountered during a let-decl, fn param, or
    // type-decl field interns lazily into one shared store. Written into
    // `module.rc_layouts` at end of `lower()`.
    let mut aliases: HashMap<String, Type> = HashMap::new();
    let mut rc_layouts: Vec<Type> = Vec::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl { name, fields } = stmt {
            let mut layout: Vec<(String, Type)> = Vec::with_capacity(fields.len());
            for (fname, fty_ann) in fields {
                let ty = parse_type(Some(fty_ann.as_str()), &aliases, &mut rc_layouts);
                layout.push((fname.clone(), ty));
            }
            let sid = module.intern_struct(layout);
            aliases.insert(name.clone(), Type::Obj(sid));
        }
    }

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
            let ret_ty = parse_type(return_type.as_deref(), &aliases, &mut rc_layouts);
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(name.clone(), fid);
            module.funcs.push(ssa::Function::new(name.clone(), ret_ty));
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
        print_f64: print_f64_id,
        str_alloc: str_alloc_id,
        str_print: str_print_id,
        str_drop: str_drop_id,
        str_concat: str_concat_id,
        obj_alloc: obj_alloc_id,
        obj_drop: obj_drop_id,
        rc_alloc: rc_alloc_id,
        rc_clone: rc_clone_id,
        rc_drop: rc_drop_id,
        math_sqrt: math_sqrt_id,
        math_abs: math_abs_id,
        math_floor: math_floor_id,
        math_ceil: math_ceil_id,
        math_log: math_log_id,
        math_exp: math_exp_id,
        math_pow: math_pow_id,
        math_min: math_min_id,
        math_max: math_max_id,
    };

    // Snapshot struct layouts BEFORE entering pass 2 (which mutates
    // module.funcs). Pass 2 only reads layouts (for object-lit field
    // sizes / member-access offsets), so a clone is safe and simpler
    // than fighting the borrow checker.
    let struct_layouts_snapshot = module.struct_layouts.clone();

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
            let string_id_base = module.strings.len();
            let (f, new_strings) = lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                &fn_table,
                &signatures,
                &intrinsics,
                &aliases,
                &mut rc_layouts,
                &struct_layouts_snapshot,
                string_id_base,
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
        let string_id_base = module.strings.len();
        let (main_fn, new_strings) = synthesize_main(
            &top_level,
            ast,
            &fn_table,
            &signatures,
            &intrinsics,
            &aliases,
            &mut rc_layouts,
            &struct_layouts_snapshot,
            string_id_base,
        );
        for s in new_strings {
            module.strings.push(s);
        }
        module.funcs.push(main_fn);
    }

    module.rc_layouts = rc_layouts;
    module
}

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
struct Intrinsics {
    print_i64: FuncId,
    print_f64: FuncId,
    str_alloc: FuncId,
    str_print: FuncId,
    str_drop: FuncId,
    str_concat: FuncId,
    obj_alloc: FuncId,
    obj_drop: FuncId,
    rc_alloc: FuncId,
    rc_clone: FuncId,
    rc_drop: FuncId,
    math_sqrt: FuncId,
    math_abs: FuncId,
    math_floor: FuncId,
    math_ceil: FuncId,
    math_log: FuncId,
    math_exp: FuncId,
    math_pow: FuncId,
    math_min: FuncId,
    math_max: FuncId,
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
    aliases: &HashMap<String, Type>,
    rc_layouts: &mut Vec<Type>,
    struct_layouts: &[Vec<(String, Type)>],
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
            aliases,
            rc_layouts,
            struct_layouts,
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

fn parse_type(
    ann: Option<&str>,
    aliases: &HashMap<String, Type>,
    rc_layouts: &mut Vec<Type>,
) -> Type {
    let s = match ann {
        Some(s) => s,
        // `number` defaults to i64 — best for the integer-heavy cases
        // (popcount/fib40/gcd1m). When a function actually needs floating-
        // point semantics, the user annotates with the explicit `f64` type
        // (Rust-shaped). The dialect lets you opt in; you don't pay the
        // f64 tax just because TS spells everything `number`.
        None => return Type::Void,
    };
    // `Rc<X>` — strip outer wrapper, recurse on inner, intern. The flat
    // string was produced by parser::parse_type_ann so the outer brackets
    // bracket exactly one inner type.
    if let Some(rest) = s.strip_prefix("Rc<")
        && let Some(inner) = rest.strip_suffix('>')
    {
        let inner_ty = parse_type(Some(inner.trim()), aliases, rc_layouts);
        let id = intern_rc_layout(rc_layouts, inner_ty);
        return Type::Rc(id);
    }
    match s {
        "number" | "i64" => Type::I64,
        "f64" => Type::F64,
        "boolean" => Type::Bool,
        "string" => Type::Str,
        "void" => Type::Void,
        other => match aliases.get(other) {
            Some(ty) => *ty,
            None => panic!("ssa-lower: unsupported type annotation `{other}`"),
        },
    }
}

fn intern_rc_layout(rc_layouts: &mut Vec<Type>, payload: Type) -> ssa::RcId {
    for (i, existing) in rc_layouts.iter().enumerate() {
        if *existing == payload {
            return ssa::RcId(i as u32);
        }
    }
    let id = ssa::RcId(rc_layouts.len() as u32);
    rc_layouts.push(payload);
    id
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
    aliases: &HashMap<String, Type>,
    rc_layouts: &mut Vec<Type>,
    struct_layouts: &[Vec<(String, Type)>],
    string_id_base: usize,
) -> (ssa::Function, Vec<Vec<u8>>) {
    let ret_ty = parse_type(return_type, aliases, rc_layouts);
    let mut f = ssa::Function::new(name, ret_ty);

    // Capture param SSA values + types BEFORE creating the entry block; we'll
    // alloca-and-store each one inside entry below so the lowerer can treat
    // params and let-locals uniformly (both read via Load, both writable via
    // Store; params just happen to be initialized from the function's
    // SSA-arg values).
    let mut param_setup: Vec<(String, ValueId, Type)> = Vec::with_capacity(params.len());
    for p in params {
        let pty = parse_type(p.type_ann.as_deref(), aliases, rc_layouts);
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
        aliases,
        rc_layouts,
        struct_layouts,
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
            InstKind::Store(Operand::Value(pid), Operand::Value(slot), 0),
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
    /// User-declared type aliases (`type Point = { ... }` → Type::Obj).
    /// Threaded through so `parse_type("Point", ...)` resolves at let-decl
    /// + function-signature sites.
    aliases: &'a HashMap<String, Type>,
    /// Mutable view of the lowering-phase Rc payload interner. let-decl
    /// annotations encountered during body lowering may introduce new
    /// `Rc<X>` instantiations; they intern lazily here. Written into
    /// `module.rc_layouts` at the very end of `lower()`.
    rc_layouts: &'a mut Vec<Type>,
    /// Read-only view of all interned struct layouts. Object-lit + member
    /// access need field-offset info, which is `field_index * 8` in MVP.
    /// This is a snapshot — passing `&module.struct_layouts` directly
    /// would conflict with the `&mut module.funcs` borrow during body
    /// lowering, so we clone the Vec at the start of pass 2.
    struct_layouts: &'a [Vec<(String, Type)>],
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
            //
            // Both `Ident(s)` and `Member { obj, name }` are borrows — the
            // Ident reads a binding's heap pointer; the Member reads a
            // field whose backing heap is still owned by the parent
            // struct. Dropping either would double-free.
            let is_borrow = matches!(
                self.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. }
            );
            let arg = self.lower_expr(args[0]);
            let arg_ty = self.operand_ty(&arg);
            let is_str = arg_ty == Type::Str;
            let target = match arg_ty {
                Type::Str => self.intrinsics.str_print,
                Type::F64 => self.intrinsics.print_f64,
                _ => self.intrinsics.print_i64,
            };
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![arg]));
            if is_str && !is_borrow {
                self.emit_drop_value(arg, Type::Str);
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

    /// Drop a single Operand of non-Copy type. Recurses into struct fields:
    ///
    ///   Str       → call str_drop(val)
    ///   Obj(sid)  → for each non-Copy field at offset i*8:
    ///                  load field, recursively drop its value;
    ///               call obj_drop(val)  // free the outer struct after
    ///                                   // its non-Copy children are gone
    ///
    /// Copy fields don't show up here — they don't own anything heap.
    /// Recursion bottoms out at Str (the leaves) or at Obj with all-Copy
    /// fields (just free, no inner drops). Cycles aren't possible because
    /// our type aliases are declaration-ordered and forward refs are
    /// rejected at the type-decl pass — there's no way to build a
    /// recursive struct.
    fn emit_drop_value(&mut self, val: Operand, ty: Type) {
        match ty {
            Type::Str => {
                let drop_fid = self.intrinsics.str_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![val]),
                );
            }
            Type::Obj(sid) => {
                let layout = self.struct_layouts[sid.0 as usize].clone();
                for (i, (_, fty)) in layout.iter().enumerate() {
                    if fty.is_copy() {
                        continue;
                    }
                    let offset = i as u64 * 8;
                    let field_val = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(*fty, val, offset),
                        *fty,
                        None,
                    );
                    self.emit_drop_value(Operand::Value(field_val), *fty);
                }
                let drop_fid = self.intrinsics.obj_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![val]),
                );
            }
            Type::Rc(rc_id) => {
                // P2.3.c — Rc drop emission. Two paths:
                //   1. Copy payload (Rc<i64> / Rc<f64> / Rc<bool>): the
                //      payload owns nothing heap, so a single
                //      `__torajs_rc_drop(p)` call (dec + free if zero)
                //      is enough. Smaller code at the drop site.
                //   2. Non-Copy payload (Rc<Str> / Rc<Obj> / Rc<Rc<T>>):
                //      we need an inline branch — only walk inner contents
                //      when the count reaches zero. Layout is `dec ; if 0
                //      { drop inner ; free }`. obj_drop is reused for the
                //      shallow free of the rc allocation since it's just
                //      `free(p)` underneath.
                //
                // Cycles in `Rc<T>` graphs leak by design (no weak refs
                // until P15+). Forward-ref-rejection at type-decl time
                // means the cycles can't form via static type aliases —
                // only via dynamic mutation, which we don't support yet.
                let payload = self.rc_layouts[rc_id.0 as usize];
                if payload.is_copy() {
                    let drop_fid = self.intrinsics.rc_drop;
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(drop_fid, vec![val]),
                    );
                    return;
                }
                // Non-Copy: emit inline dec + branch + inner drop + free.
                let strong = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, val, 0),
                    Type::I64,
                    None,
                );
                let strong_dec = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Sub,
                        Operand::Value(strong),
                        Operand::ConstI64(1),
                    ),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(strong_dec), val, 0),
                );
                let is_zero = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(
                        IPred::Eq,
                        Operand::Value(strong_dec),
                        Operand::ConstI64(0),
                    ),
                    Type::Bool,
                    None,
                );
                let then_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(is_zero),
                        then_blk,
                        else_blk: after_blk,
                    },
                );

                self.cur_block = then_blk;
                // Drop the payload's inner non-Copy contents. The payload
                // sits at byte offset 16; for Obj fields each non-Copy
                // field is at 16 + i*8. For Str / nested Rc the single
                // pointer lives at offset 16.
                match payload {
                    Type::Str => {
                        let inner = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::Str, val, 16),
                            Type::Str,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.str_drop,
                                vec![Operand::Value(inner)],
                            ),
                        );
                    }
                    Type::Obj(sid) => {
                        let layout = self.struct_layouts[sid.0 as usize].clone();
                        for (i, (_, fty)) in layout.iter().enumerate() {
                            if fty.is_copy() {
                                continue;
                            }
                            let offset = 16 + i as u64 * 8;
                            let inner_val = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(*fty, val, offset),
                                *fty,
                                None,
                            );
                            self.emit_drop_value(Operand::Value(inner_val), *fty);
                        }
                        // Do NOT call obj_drop on the inline struct — its
                        // memory is part of the Rc allocation (freed below).
                    }
                    Type::Rc(_) => {
                        let inner_rc = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(payload, val, 16),
                            payload,
                            None,
                        );
                        self.emit_drop_value(Operand::Value(inner_rc), payload);
                    }
                    _ => {
                        // Should be unreachable since we already checked
                        // is_copy above. Defensive panic.
                        panic!(
                            "ssa-lower: unexpected non-Copy Rc payload {payload:?}"
                        );
                    }
                }
                // Free the rc allocation (header + inline payload). obj_drop
                // is just libc free underneath — same heap as rc_alloc used.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.obj_drop, vec![val]),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                self.cur_block = after_blk;
            }
            other if other.is_copy() => {
                // Nothing to drop — caller filtered, but be defensive.
            }
            other => panic!("ssa-lower: no drop sequence for type {other:?}"),
        }
    }

    /// P2.3.b — lower `Rc.new(value)`. Allocates `16 + payload_size` bytes
    /// via `__torajs_rc_alloc` (init strong=1 weak=0 inside the runtime),
    /// stores the payload at byte offset 16, returns a Type::Rc operand.
    ///
    /// For Obj payloads the source struct's fields are copied into the Rc
    /// inline (no extra indirection); the temporary Obj heap is freed via
    /// shallow `__torajs_obj_drop` since its non-Copy fields have moved
    /// into the Rc payload. The source binding (if any) is marked moved.
    fn lower_rc_new(&mut self, value_eid: ExprId) -> Operand {
        let val = self.lower_expr(value_eid);
        let val_ty = self.operand_ty(&val);
        let rc_id = intern_rc_layout(self.rc_layouts, val_ty);
        let payload_size: i64 = match val_ty {
            Type::Obj(sid) => self.struct_layouts[sid.0 as usize].len() as i64 * 8,
            _ => 8,
        };
        // Runtime adds the 16-byte strong+weak header internally — caller
        // hands it the payload size only (matches roadmap contract).
        let rc_ptr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.rc_alloc,
                vec![Operand::ConstI64(payload_size)],
            ),
            Type::Rc(rc_id),
            None,
        );
        match val_ty {
            Type::Obj(sid) => {
                // Field-by-field copy from source obj into Rc payload.
                let layout = self.struct_layouts[sid.0 as usize].clone();
                for (i, (_, fty)) in layout.iter().enumerate() {
                    let field_offset = i as u64 * 8;
                    let field_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(*fty, val, field_offset),
                        *fty,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(field_v),
                            Operand::Value(rc_ptr),
                            16 + field_offset,
                        ),
                    );
                }
                // Free the source obj's outer struct shallowly — its
                // non-Copy field heaps have transferred ownership to the
                // Rc payload via the field copies above.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.obj_drop, vec![val]),
                );
            }
            _ => {
                // Primitive / Str / Rc — single 8-byte slot at offset 16.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(val, Operand::Value(rc_ptr), 16),
                );
            }
        }
        // Mark source binding moved (no-op for literals / Copy types).
        self.consume_if_ident(value_eid);
        Operand::Value(rc_ptr)
    }

    /// Emit drop sequences for every owned non-Copy local in the current
    /// block. Called immediately before terminators that exit the function
    /// (Ret, fall-through). Skips `moved` bindings — those have transferred
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
        for (slot, ty) in to_drop {
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(slot), 0),
                ty,
                None,
            );
            self.emit_drop_value(Operand::Value(val), ty);
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
                let ty = parse_type(type_ann.as_deref(), self.aliases, self.rc_layouts);
                let init_val = self.lower_expr(*init);
                self.consume_if_ident(*init);
                // Coerce init to the declared slot type if needed.
                // Currently only i64 → f64 promotion shows up (literals like
                // `2.0` lower as ConstI64 because they have no fractional
                // part; the slot annotation `f64` then forces the cast).
                let init_val = if ty == Type::F64 && self.operand_ty(&init_val) == Type::I64 {
                    self.coerce_to_f64(init_val)
                } else {
                    init_val
                };
                let slot = self.alloca(ty, Some(name));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(init_val, Operand::Value(slot), 0),
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
            Stmt::TypeDecl { .. } => {
                // Pass 0 of `lower()` already registered the alias and
                // interned the layout. Re-encountering during the body
                // walk is a no-op.
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
                    InstKind::Load(info.ty, Operand::Value(info.slot), 0),
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
                let snapshot = match self.locals.get(&name) {
                    Some(i) => *i,
                    None => panic!("ssa-lower: assign to unknown ident `{name}`"),
                };
                // Lower the rhs FIRST. The rhs might internally consume the
                // lhs binding (e.g. `s = s + "x"` — concat takes ownership
                // of s, freeing its heap). Once consumed, the slot's
                // pointer is dangling — we must NOT load+drop it as the
                // "old value" at that point.
                let v = self.lower_expr(*value);
                self.consume_if_ident(*value);
                // Now check if the lhs binding is *still* owned (rhs didn't
                // consume it). If yes, the slot still holds a live heap
                // pointer that needs freeing before we overwrite. If
                // moved, the rhs's flow already disposed of the heap.
                let post_rhs = *self.locals.get(&name).unwrap_or(&snapshot);
                if !snapshot.ty.is_copy() && !post_rhs.moved {
                    let old = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(snapshot.ty, Operand::Value(snapshot.slot), 0),
                        snapshot.ty,
                        None,
                    );
                    self.emit_drop_value(Operand::Value(old), snapshot.ty);
                }
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(v, Operand::Value(snapshot.slot), 0),
                );
                // The slot now owns a fresh value — clear `moved` so
                // subsequent reads work and end-of-fn drop fires.
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
                // P2.3.b — Rc.new(value): result type is Rc<T> where T is
                // the arg's type. Special-cased here because the callee
                // resolver can't synthesize a generic FuncId.
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee).clone()
                    && name == "new"
                    && matches!(self.ast.get_expr(obj), Expr::Ident(n) if n == "Rc")
                    && !self.locals.contains_key("Rc")
                {
                    debug_assert_eq!(args.len(), 1, "check.rs guarantees Rc.new arity");
                    return self.lower_rc_new(args[0]);
                }
                // P2.3.b — receiver.clone() on Rc<T>: emit __torajs_rc_clone,
                // do NOT consume the receiver (read borrow).
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee).clone()
                    && name == "clone"
                    && args.is_empty()
                {
                    let recv = self.lower_expr(obj);
                    let recv_ty = self.operand_ty(&recv);
                    if matches!(recv_ty, Type::Rc(_)) {
                        let cloned = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.rc_clone, vec![recv]),
                            recv_ty,
                            None,
                        );
                        return Operand::Value(cloned);
                    }
                    panic!(
                        "ssa-lower: .clone() on non-Rc type {recv_ty:?} not supported (check.rs should have rejected)"
                    );
                }
                let target = self.resolve_callee(*callee);
                let mut argv: Vec<Operand> =
                    args.iter().map(|a| self.lower_expr(*a)).collect();
                // Consume non-Copy ident args. Mirrors check.rs's pass; the
                // SSA layer needs the same flag so end-of-fn drops skip
                // moved bindings.
                for a in args {
                    self.consume_if_ident(*a);
                }
                // Coerce arguments to the callee's expected param types.
                // Currently only f64-promotion is needed (Math.* takes
                // f64; users may pass integer expressions like `Math.sqrt(2)`).
                // Look up callee's param types from `module.funcs[target]`'s
                // signature — but we don't have a Module borrow here.
                // Instead, snapshot the callee's params at signature-build
                // time. For now, the only intrinsic with non-Any non-self-
                // type params that needs coercion is Math.*; treat them
                // specially by FuncId.
                if self.is_math_unary(target) {
                    debug_assert_eq!(argv.len(), 1, "Math.* unary takes 1 arg");
                    argv[0] = self.coerce_to_f64(argv[0]);
                } else if self.is_math_binary(target) {
                    debug_assert_eq!(argv.len(), 2, "Math.* binary takes 2 args");
                    argv[0] = self.coerce_to_f64(argv[0]);
                    argv[1] = self.coerce_to_f64(argv[1]);
                }
                let ret_ty = self.f_ret_type_hint(target);
                let v = self
                    .f
                    .append_inst(self.cur_block, InstKind::Call(target, argv), ret_ty, None);
                Operand::Value(v)
            }
            Expr::ObjectLit { fields } => {
                // Lower each field. Field types determine struct layout
                // (8-byte slots in declaration order for MVP). Match the
                // already-interned StructId by structural equality on
                // the layout — this is what `intern_struct` did in pass
                // 0.5 for `type` aliases. For object literals appearing
                // without a `type` declaration in scope, the layout is
                // inferred and would intern as a fresh struct (but we
                // don't have a way to register new structs into the
                // module post-pass-0 — the layout snapshot is read-only).
                //
                // For MVP we require: every object literal must match the
                // layout of an already-declared `type`. The check.rs
                // pass infers structurally; the SSA lowerer matches
                // against `struct_layouts` registered in pass 0.5.
                let entries: Vec<(String, ExprId)> = fields.clone();
                let mut field_tys: Vec<(String, Type)> = Vec::new();
                let mut field_vals: Vec<Operand> = Vec::new();
                for (n, eid) in &entries {
                    let v = self.lower_expr(*eid);
                    self.consume_if_ident(*eid);
                    let ty = self.operand_ty(&v);
                    field_tys.push((n.clone(), ty));
                    field_vals.push(v);
                }
                let sid = self
                    .struct_layouts
                    .iter()
                    .position(|layout| *layout == field_tys)
                    .map(|i| ssa::StructId(i as u32))
                    .unwrap_or_else(|| {
                        panic!(
                            "ssa-lower: object literal layout {field_tys:?} not registered as a `type` — anonymous struct types not yet supported (P2.4.c MVP)"
                        )
                    });
                let size = field_tys.len() as i64 * 8;
                let alloc_fid = self.intrinsics.obj_alloc;
                let obj_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(alloc_fid, vec![Operand::ConstI64(size)]),
                    Type::Obj(sid),
                    None,
                );
                for (i, val) in field_vals.iter().enumerate() {
                    let offset = i as u64 * 8;
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(*val, Operand::Value(obj_ptr), offset),
                    );
                }
                Operand::Value(obj_ptr)
            }
            Expr::Member { obj, name } => {
                // `Math.PI` and `Math.E` are compile-time constants — no
                // SSA value at all, just synthesize a ConstF64 operand.
                if let Expr::Ident(n) = self.ast.get_expr(*obj)
                    && n == "Math"
                {
                    return match name.as_str() {
                        "PI" => Operand::ConstF64(std::f64::consts::PI),
                        "E" => Operand::ConstF64(std::f64::consts::E),
                        other => panic!("ssa-lower: unknown Math constant `{other}`"),
                    };
                }
                let obj_val = self.lower_expr(*obj);
                let obj_ty = self.operand_ty(&obj_val);
                // `s.length` for Type::Str — read the u64 length stored
                // at offset 0 of the StrRepr.
                if obj_ty == Type::Str && name == "length" {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, obj_val, 0),
                        Type::I64,
                        None,
                    );
                    return Operand::Value(v);
                }
                // P2.3.b — auto-deref `Rc<Obj>` for field access. The
                // payload sits at byte offset 16 (past the strong+weak
                // header), so each field's offset is 16 + idx*8.
                if let Type::Rc(rc_id) = obj_ty {
                    let payload = self.rc_layouts[rc_id.0 as usize];
                    if let Type::Obj(sid) = payload {
                        let layout = &self.struct_layouts[sid.0 as usize];
                        let (idx, fty) = layout
                            .iter()
                            .enumerate()
                            .find_map(|(i, (fname, ft))| {
                                if fname == name {
                                    Some((i, *ft))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "ssa-lower: Rc<Obj({sid:?})> has no field `{name}`"
                                )
                            });
                        let offset = 16 + idx as u64 * 8;
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(fty, obj_val, offset),
                            fty,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    panic!(
                        "ssa-lower: member access on Rc<{payload:?}> — only Rc<Obj> supports field access in P2.3.b"
                    );
                }
                let sid = match obj_ty {
                    Type::Obj(sid) => sid,
                    _ => panic!(
                        "ssa-lower: member access on non-object {obj_ty:?} (.{name})"
                    ),
                };
                let layout = &self.struct_layouts[sid.0 as usize];
                let (idx, field_ty) = layout
                    .iter()
                    .enumerate()
                    .find_map(|(i, (fname, fty))| {
                        if fname == name {
                            Some((i, *fty))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "ssa-lower: struct {sid:?} has no field `{name}` (layout: {layout:?})"
                        )
                    });
                let offset = idx as u64 * 8;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(field_ty, obj_val, offset),
                    field_ty,
                    None,
                );
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
            // Member call — currently only `Math.<method>` resolves here.
            // `console.log(...)` is handled by the top-level shortcut in
            // `lower_top_stmt`, so it never reaches here as a regular Call.
            Expr::Member { obj, name } => {
                let is_math = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Math");
                if is_math {
                    return match name.as_str() {
                        "sqrt" => self.intrinsics.math_sqrt,
                        "abs" => self.intrinsics.math_abs,
                        "floor" => self.intrinsics.math_floor,
                        "ceil" => self.intrinsics.math_ceil,
                        "log" => self.intrinsics.math_log,
                        "exp" => self.intrinsics.math_exp,
                        "pow" => self.intrinsics.math_pow,
                        "min" => self.intrinsics.math_min,
                        "max" => self.intrinsics.math_max,
                        other => {
                            panic!("ssa-lower: unknown Math method `{other}`")
                        }
                    };
                }
                panic!("ssa-lower: unsupported member call shape: {name}")
            }
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }

    fn is_math_unary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_sqrt
            || fid == self.intrinsics.math_abs
            || fid == self.intrinsics.math_floor
            || fid == self.intrinsics.math_ceil
            || fid == self.intrinsics.math_log
            || fid == self.intrinsics.math_exp
    }

    fn is_math_binary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_pow
            || fid == self.intrinsics.math_min
            || fid == self.intrinsics.math_max
    }

    /// Look up the callee's return type from the signatures map populated
    /// in pass 1 of `lower`. Defaults to I64 for unknown FuncIds (intrinsics
    /// or forward refs we haven't catalogued yet — print_i64 returns void
    /// and is called via `append_void`, so its callsites never reach here).
    fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }
}
