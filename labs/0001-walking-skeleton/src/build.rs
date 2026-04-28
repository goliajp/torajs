//! AOT to wasm — currently P3.1 → P3.3.
//!
//! Walks the AST directly (NOT the IR) and emits a type-specialized wasm
//! module. The interpreter still consumes the IR; the two paths diverge here.
//! TODO: make the IR carry structured control flow (if / loop / block) so
//! both paths share it again — the roadmap's "share IR with AOT" rule.
//!
//! Supported (P3.3):
//!   - top-level `console.log("<lit>")` — fd_write of static bytes
//!   - top-level `console.log(<number-expr>)` — calls `print_i64` helper
//!   - `function`s with `number` params and `number`/`void` return
//!   - if/return/recursion, number arithmetic, ordering comparisons,
//!     strict equality on numbers
//!
//! Not yet (returns NotYetImplemented):
//!   - `let`/`const` (locals beyond fn params)
//!   - `while`, blocks introducing new bindings
//!   - strings beyond a single literal in console.log
//!   - arrays, arrow fns, assignment, string concat
//!
//! Number representation: f64. fib40-style integer math runs via f64 ops;
//! `print_i64` truncates to i64 (sat) before formatting decimal digits.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, ImportSection, MemArg, MemorySection, MemoryType, Module,
    TypeSection, ValType,
};

use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};

/// AOT build outcome. Exit codes (3 vs 1) let the bench harness distinguish
/// "torajs-aot doesn't support this program shape yet" (skip) from real bugs.
#[derive(Debug)]
pub enum BuildError {
    NotYetImplemented(String),
    Real(String),
}

// Memory layout (single 1-page = 64 KiB linear memory):
//   0..40    digit buffer for `print_i64` (newline lives at byte 39)
//   40..48   iovec { ptr: i32, len: i32 }
//   48..52   nwritten output slot for fd_write
//   64..     static data section (string literals committed by codegen)
const MEM_DIGIT_BUF_END: u32 = 39; // last byte (newline) of digit buffer
const MEM_IOVEC: u32 = 40;
const MEM_NWRITTEN: u32 = 48;
const MEM_STATIC_START: u32 = 64;

// Function index plan:
//   0  = imported fd_write
//   1  = print_i64 helper
//   2  = print_static helper (ptr, len)
//   3+ = user functions, declared in source order
const FN_FD_WRITE: u32 = 0;
const FN_PRINT_I64: u32 = 1;
const FN_PRINT_STATIC: u32 = 2;
const FN_USER_BASE: u32 = 3;

// Wasm type indices:
//   0 = (i32 i32 i32 i32) -> i32         (fd_write, print_static body args)
//   1 = ()                                (helpers / void user fns)
//   2 = (f64) -> ()                       (print_i64)
//   3 = (i32 i32) -> ()                   (print_static)
//   4+ = user fn signatures
const TYPE_FD_WRITE: u32 = 0;
const TYPE_VOID: u32 = 1;
const TYPE_PRINT_I64: u32 = 2;
const TYPE_PRINT_STATIC: u32 = 3;
const TYPE_USER_BASE: u32 = 4;

pub fn build(ast: &Ast, out_path: &Path) -> Result<(), BuildError> {
    let mut compiler = Compiler::new(ast);
    compiler.collect_globals()?;
    compiler.emit_module()?;
    let wasm = compiler.module.finish();
    fs::write(out_path, &wasm)
        .map_err(|e| BuildError::Real(format!("writing {}: {e}", out_path.display())))?;
    Ok(())
}

/// Module-wide numeric mode. Set once during `Compiler::new` by walking the
/// AST: if no division and no fractional number literals appear anywhere,
/// every `number` lowers as `i64` (faster integer pipeline + no f64→i64
/// truncation at print). Otherwise we keep `f64` (correct JS semantics).
///
/// This is the cheap-and-correct version of type narrowing — the static
/// type system says `number = f64`, but when the AST proves the program
/// only does integer-preserving arithmetic, the wasm path can stay in i64.
/// fib40 trips into i64 mode; anything with `/` or `1.5` stays in f64.
#[derive(Debug, Clone, Copy, PartialEq)]
enum NumericMode {
    F64,
    I64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum WasmTy {
    F64,
    I64,
    I32,
    Void,
}

impl WasmTy {
    fn from_ann(ann: &str, mode: NumericMode) -> Option<WasmTy> {
        match ann {
            // `number` flexes with the program-wide numeric mode (set by
            // `detect_numeric_mode` from the AST). `i64` and `f64` are the
            // explicit Rust-shaped aliases — `f64` always wins over the
            // detected mode for that specific binding, but the wasm-via-C
            // path is global-mode, so accepting `f64` here is enough only
            // when the whole module is already in F64 mode (mandelbrot is).
            "number" => Some(match mode {
                NumericMode::F64 => WasmTy::F64,
                NumericMode::I64 => WasmTy::I64,
            }),
            "f64" => Some(WasmTy::F64),
            "i64" => Some(WasmTy::I64),
            "boolean" => Some(WasmTy::I32),
            "void" => Some(WasmTy::Void),
            _ => None,
        }
    }

    fn val_type(self) -> Option<ValType> {
        match self {
            WasmTy::F64 => Some(ValType::F64),
            WasmTy::I64 => Some(ValType::I64),
            WasmTy::I32 => Some(ValType::I32),
            WasmTy::Void => None,
        }
    }
}

struct UserFn {
    name: String,
    params: Vec<(String, WasmTy)>,
    ret: WasmTy,
    /// Wasm function index (3+).
    index: u32,
    /// Wasm type index (4+).
    type_index: u32,
}

struct Compiler<'a> {
    ast: &'a Ast,
    module: Module,
    /// Static string data committed to the data section.
    /// Each entry: (offset, bytes_with_newline).
    static_strings: Vec<(u32, Vec<u8>)>,
    /// Next free byte offset for new static data.
    static_cursor: u32,
    /// Top-level user function declarations (excludes `main`).
    user_fns: HashMap<String, UserFn>,
    /// Order in which user fns were declared (so we emit code in matching order).
    user_fn_order: Vec<String>,
    /// Resolved at construction by walking the AST.
    numeric_mode: NumericMode,
}

impl<'a> Compiler<'a> {
    fn new(ast: &'a Ast) -> Self {
        let numeric_mode = detect_numeric_mode(ast);
        Self {
            ast,
            module: Module::new(),
            static_strings: Vec::new(),
            static_cursor: MEM_STATIC_START,
            user_fns: HashMap::new(),
            user_fn_order: Vec::new(),
            numeric_mode,
        }
    }

    /// First pass: collect top-level fn signatures into `user_fns`.
    fn collect_globals(&mut self) -> Result<(), BuildError> {
        for stmt in &self.ast.stmts {
            if let Stmt::FnDecl {
                name,
                params,
                return_type,
                ..
            } = stmt
            {
                let mut wparams = Vec::new();
                for p in params {
                    let ann = p.type_ann.as_deref().ok_or_else(|| {
                        BuildError::Real(format!(
                            "fn `{name}` param `{}` has no type annotation; check.rs should have caught this",
                            p.name
                        ))
                    })?;
                    let ty = WasmTy::from_ann(ann, self.numeric_mode).ok_or_else(|| {
                        BuildError::NotYetImplemented(format!(
                            "fn `{name}` param `{}` has type `{ann}` — AOT only supports number/boolean/void today",
                            p.name
                        ))
                    })?;
                    if matches!(ty, WasmTy::Void) {
                        return Err(BuildError::Real(format!(
                            "fn `{name}` param `{}` is void",
                            p.name
                        )));
                    }
                    wparams.push((p.name.clone(), ty));
                }
                let ret = match return_type {
                    None => WasmTy::Void,
                    Some(ann) => WasmTy::from_ann(ann, self.numeric_mode).ok_or_else(|| {
                        BuildError::NotYetImplemented(format!(
                            "fn `{name}` return type `{ann}` — AOT only supports number/boolean/void today"
                        ))
                    })?,
                };
                let index = FN_USER_BASE + self.user_fn_order.len() as u32;
                let type_index = TYPE_USER_BASE + self.user_fn_order.len() as u32;
                self.user_fn_order.push(name.clone());
                self.user_fns.insert(
                    name.clone(),
                    UserFn {
                        name: name.clone(),
                        params: wparams,
                        ret,
                        index,
                        type_index,
                    },
                );
            }
        }
        Ok(())
    }

    fn emit_module(&mut self) -> Result<(), BuildError> {
        // .types — hardcoded slots 0..4, then one per user fn
        let mut types = TypeSection::new();
        types.ty().function(
            [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            [ValType::I32],
        );
        types.ty().function([], []);
        let print_param = match self.numeric_mode {
            NumericMode::F64 => ValType::F64,
            NumericMode::I64 => ValType::I64,
        };
        types.ty().function([print_param], []);
        types.ty().function([ValType::I32, ValType::I32], []);
        for name in &self.user_fn_order {
            let f = &self.user_fns[name];
            let params: Vec<ValType> = f
                .params
                .iter()
                .map(|(_, t)| t.val_type().expect("non-void param"))
                .collect();
            let results: Vec<ValType> = f.ret.val_type().into_iter().collect();
            types.ty().function(params, results);
        }
        self.module.section(&types);

        // .imports — wasi_snapshot_preview1.fd_write only
        let mut imports = ImportSection::new();
        imports.import(
            "wasi_snapshot_preview1",
            "fd_write",
            EntityType::Function(TYPE_FD_WRITE),
        );
        self.module.section(&imports);

        // .functions — print_i64, print_static, user fns, _start
        let mut functions = FunctionSection::new();
        functions.function(TYPE_PRINT_I64); // print_i64
        functions.function(TYPE_PRINT_STATIC); // print_static
        for name in &self.user_fn_order {
            functions.function(self.user_fns[name].type_index);
        }
        functions.function(TYPE_VOID); // _start
        self.module.section(&functions);

        // .memory — 1 page
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: 1,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        self.module.section(&memories);

        // .exports — memory + _start
        let mut exports = ExportSection::new();
        exports.export("memory", ExportKind::Memory, 0);
        let start_index = FN_USER_BASE + self.user_fn_order.len() as u32;
        exports.export("_start", ExportKind::Func, start_index);
        self.module.section(&exports);

        // .code — print_i64, print_static, user fns, _start
        let mut code = CodeSection::new();
        code.function(&emit_print_i64(self.numeric_mode));
        code.function(&emit_print_static());
        for name in &self.user_fn_order.clone() {
            let f = self.compile_user_fn(name)?;
            code.function(&f);
        }
        let main_fn = self.compile_main()?;
        code.function(&main_fn);
        self.module.section(&code);

        // .data — pre-bake the iovec slot once (`print_static` overwrites it
        // each call, but having 8 zeros there avoids referencing uninitialized
        // memory in a debugger). Plus all static strings.
        let mut data = DataSection::new();
        let zero_iovec = vec![0u8; 12]; // iovec(8) + nwritten(4)
        data.active(0, &ConstExpr::i32_const(MEM_IOVEC as i32), zero_iovec);
        for (offset, bytes) in &self.static_strings {
            data.active(0, &ConstExpr::i32_const(*offset as i32), bytes.clone());
        }
        self.module.section(&data);

        Ok(())
    }

    fn compile_user_fn(&mut self, name: &str) -> Result<Function, BuildError> {
        let stmt = self
            .ast
            .stmts
            .iter()
            .find(|s| matches!(s, Stmt::FnDecl { name: n, .. } if n == name))
            .ok_or_else(|| BuildError::Real(format!("fn `{name}` decl not found")))?;
        let Stmt::FnDecl { body, .. } = stmt else {
            unreachable!();
        };
        let info = self.user_fns[name].clone_signature();
        let body_clone = body.clone();
        self.compile_function_body(info, body_clone)
    }

    fn compile_main(&mut self) -> Result<Function, BuildError> {
        let info = SignatureRef {
            params: Vec::new(),
            ret: WasmTy::Void,
        };
        let stmts: Vec<Stmt> = self
            .ast
            .stmts
            .iter()
            .filter(|s| !matches!(s, Stmt::FnDecl { .. }))
            .cloned()
            .collect();
        self.compile_function_body(info, stmts)
    }

    /// Walks a function body (or main) twice:
    ///   pass 1 — discover top-level `let`/`const` bindings and their wasm
    ///            types so all locals can be declared upfront in the wasm
    ///            function header (wasm-encoder requires it).
    ///   pass 2 — actually lower statements; `LetDecl` looks up the slot
    ///            already reserved.
    /// Lets nested inside `if`/`while`/`block` bodies are not supported yet
    /// (would need proper scope-stack-with-slot-reuse). Detected during
    /// pass 1 and rejected as `NotYetImplemented`.
    fn compile_function_body(
        &mut self,
        info: SignatureRef,
        body: Vec<Stmt>,
    ) -> Result<Function, BuildError> {
        // Pass 1: collect top-level lets.
        let arity = info.params.len() as u32;
        let mut params_map: HashMap<String, WasmTy> = HashMap::new();
        for (n, t) in &info.params {
            params_map.insert(n.clone(), *t);
        }
        let mut declared_lets: Vec<(String, WasmTy)> = Vec::new();
        for stmt in &body {
            // detect any nested let; reject early with a clear message
            self.check_no_nested_let(stmt)?;
            if let Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } = stmt
            {
                let ty = match type_ann {
                    Some(ann) => WasmTy::from_ann(ann, self.numeric_mode).ok_or_else(|| {
                        BuildError::NotYetImplemented(format!(
                            "let `{name}` has unsupported type annotation `{ann}`"
                        ))
                    })?,
                    None => self.infer_wasm_type(*init, &params_map, &declared_lets)?,
                };
                if matches!(ty, WasmTy::Void) {
                    return Err(BuildError::Real(format!("let `{name}` is void")));
                }
                declared_lets.push((name.clone(), ty));
            }
        }

        // Wasm function header: one (count=1, type) per let.
        let local_decls: Vec<(u32, ValType)> = declared_lets
            .iter()
            .map(|(_, ty)| (1u32, ty.val_type().expect("non-void let")))
            .collect();
        let mut fb = FnBuilder::new(info.clone(), &local_decls);

        // Bind params (slots 0..arity) and lets (slots arity..arity+lets.len()).
        for (i, (pname, ty)) in info.params.iter().enumerate() {
            fb.add_binding(pname.clone(), *ty, i as u32);
        }
        for (i, (lname, lty)) in declared_lets.iter().enumerate() {
            fb.add_binding(lname.clone(), *lty, arity + i as u32);
        }

        // Pass 2: real lowering.
        for s in &body {
            fb.lower_stmt(self, s)?;
        }
        fb.function.instructions().end();
        Ok(fb.into_function())
    }

    /// Recursively checks that no `let`/`const` decl appears inside a nested
    /// block, if-branch, or while-body. Top-level lets in the function body
    /// are fine. Returns `NotYetImplemented` if a nested let is found.
    fn check_no_nested_let(&self, stmt: &Stmt) -> Result<(), BuildError> {
        fn rec(s: &Stmt, depth: u32) -> Result<(), BuildError> {
            match s {
                Stmt::LetDecl { name, .. } if depth > 0 => {
                    Err(BuildError::NotYetImplemented(format!(
                        "nested `let`/`const` binding `{name}` inside block — AOT only supports lets at the top of a function body for now (hoist them out)"
                    )))
                }
                Stmt::Block(ss) => {
                    for s in ss {
                        rec(s, depth + 1)?;
                    }
                    Ok(())
                }
                Stmt::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    rec(then_branch, depth + 1)?;
                    if let Some(eb) = else_branch {
                        rec(eb, depth + 1)?;
                    }
                    Ok(())
                }
                Stmt::While { body, .. } => rec(body, depth + 1),
                _ => Ok(()),
            }
        }
        // Top-level lets are fine; only nested ones are rejected.
        match stmt {
            Stmt::LetDecl { .. } => Ok(()),
            _ => rec(stmt, 0),
        }
    }

    fn infer_wasm_type(
        &self,
        eid: ExprId,
        params: &HashMap<String, WasmTy>,
        lets: &[(String, WasmTy)],
    ) -> Result<WasmTy, BuildError> {
        match self.ast.get_expr(eid) {
            Expr::Number(_) => Ok(match self.numeric_mode {
                NumericMode::F64 => WasmTy::F64,
                NumericMode::I64 => WasmTy::I64,
            }),
            Expr::Bool(_) => Ok(WasmTy::I32),
            Expr::Ident(name) => {
                if let Some(t) = params.get(name) {
                    return Ok(*t);
                }
                if let Some((_, t)) = lets.iter().find(|(n, _)| n == name) {
                    return Ok(*t);
                }
                Err(BuildError::NotYetImplemented(format!(
                    "ident `{name}` for let-init type inference"
                )))
            }
            Expr::BinOp { op, left, .. } => {
                let lt = self.infer_wasm_type(*left, params, lets)?;
                match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr => Ok(lt),
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Neq => {
                        Ok(WasmTy::I32)
                    }
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(fname) = self.ast.get_expr(*callee)
                    && let Some(uf) = self.user_fns.get(fname)
                {
                    return Ok(uf.ret);
                }
                Err(BuildError::NotYetImplemented(
                    "call return type for let-init inference".into(),
                ))
            }
            Expr::Assign { value, .. } => self.infer_wasm_type(*value, params, lets),
            _ => Err(BuildError::NotYetImplemented(
                "expression shape for let-init type inference".into(),
            )),
        }
    }

    /// Intern a literal string (with newline) into the static data section,
    /// return (ptr, len_with_newline).
    fn intern_string(&mut self, s: &str) -> (u32, u32) {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(b'\n');
        let offset = self.static_cursor;
        let len = bytes.len() as u32;
        self.static_strings.push((offset, bytes));
        self.static_cursor += len;
        // align cursor to 4 bytes for any future stores
        self.static_cursor = (self.static_cursor + 3) & !3;
        (offset, len)
    }
}

#[derive(Clone)]
#[allow(dead_code)] // ret/sig kept for symmetry; will read once non-void user fns return data through `_start`'s glue.
struct SignatureRef {
    params: Vec<(String, WasmTy)>,
    ret: WasmTy,
}

impl UserFn {
    fn clone_signature(&self) -> SignatureRef {
        SignatureRef {
            params: self.params.clone(),
            ret: self.ret,
        }
    }
}

/// Per-function builder. Owns a `Function` (wasm code body) and tracks its
/// local table — which mirrors the order of param declarations and any
/// future user-introduced locals.
#[allow(dead_code)] // sig kept for future use (e.g. typing `return` against the declared ret).
struct FnBuilder {
    sig: SignatureRef,
    function: Function,
    /// scope stack of name → (local_index, wasm type)
    scopes: Vec<HashMap<String, (u32, WasmTy)>>,
}

impl FnBuilder {
    fn new(sig: SignatureRef, locals: &[(u32, ValType)]) -> Self {
        Self {
            sig,
            function: Function::new(locals.iter().copied()),
            scopes: vec![HashMap::new()],
        }
    }

    fn add_binding(&mut self, name: String, ty: WasmTy, idx: u32) {
        self.scopes
            .last_mut()
            .expect("scope")
            .insert(name, (idx, ty));
    }

    fn lookup(&self, name: &str) -> Option<(u32, WasmTy)> {
        for s in self.scopes.iter().rev() {
            if let Some(&v) = s.get(name) {
                return Some(v);
            }
        }
        None
    }

    fn into_function(self) -> Function {
        self.function
    }

    fn lower_stmt(&mut self, c: &mut Compiler, stmt: &Stmt) -> Result<(), BuildError> {
        match stmt {
            Stmt::Expr(eid) => {
                let ty = self.lower_expr(c, *eid)?;
                // Discard the value if it pushes one — but console.log is void
                // and most `Stmt::Expr` we lower today is console.log.
                if !matches!(ty, WasmTy::Void) {
                    self.function.instructions().drop();
                }
                Ok(())
            }
            Stmt::Return(maybe) => {
                if let Some(eid) = maybe {
                    self.lower_expr(c, *eid)?;
                }
                self.function.instructions().return_();
                Ok(())
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let ct = self.lower_expr(c, *cond)?;
                if !matches!(ct, WasmTy::I32) {
                    return Err(BuildError::Real(format!(
                        "if condition has wasm type {ct:?}, expected I32 (boolean)"
                    )));
                }
                self.function.instructions().if_(BlockType::Empty);
                self.lower_stmt(c, then_branch)?;
                if let Some(eb) = else_branch {
                    self.function.instructions().else_();
                    self.lower_stmt(c, eb)?;
                }
                self.function.instructions().end();
                Ok(())
            }
            Stmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                let mut err = None;
                for s in stmts {
                    if let Err(e) = self.lower_stmt(c, s) {
                        err = Some(e);
                        break;
                    }
                }
                self.scopes.pop();
                match err {
                    Some(e) => Err(e),
                    None => Ok(()),
                }
            }
            Stmt::FnDecl { name, .. } => Err(BuildError::NotYetImplemented(format!(
                "nested fn `{name}` — AOT only handles top-level functions"
            ))),
            Stmt::LetDecl { name, init, .. } => {
                // Slot was reserved during pass 1; just emit init + local.set.
                let init_eid = *init;
                let init_ty = self.lower_expr(c, init_eid)?;
                let (slot, ty) = self.lookup(name).ok_or_else(|| {
                    BuildError::Real(format!("let `{name}` slot not pre-allocated"))
                })?;
                if init_ty != ty {
                    return Err(BuildError::Real(format!(
                        "let `{name}` init type {init_ty:?} != declared {ty:?}"
                    )));
                }
                self.function.instructions().local_set(slot);
                Ok(())
            }
            Stmt::While { cond, body } => {
                // wasm structured loop:
                //
                //   block $exit
                //     loop $top
                //       cond → i32
                //       i32.eqz       ; invert
                //       br_if $exit   ; if !cond, leave
                //       <body>
                //       br $top       ; repeat
                //     end
                //   end
                self.function.instructions().block(BlockType::Empty);
                self.function.instructions().loop_(BlockType::Empty);
                let ct = self.lower_expr(c, *cond)?;
                if !matches!(ct, WasmTy::I32) {
                    return Err(BuildError::Real(format!(
                        "while condition has wasm type {ct:?}, expected I32 (boolean)"
                    )));
                }
                self.function.instructions().i32_eqz();
                // br relative depth: 0 = current loop, 1 = enclosing block ($exit)
                self.function.instructions().br_if(1);
                self.lower_stmt(c, body)?;
                self.function.instructions().br(0); // continue loop
                self.function.instructions().end(); // close loop
                self.function.instructions().end(); // close block
                Ok(())
            }
        }
    }

    fn lower_expr(&mut self, c: &mut Compiler, eid: ExprId) -> Result<WasmTy, BuildError> {
        match c.ast.get_expr(eid) {
            Expr::Number(n) => match c.numeric_mode {
                NumericMode::F64 => {
                    self.function.instructions().f64_const((*n).into());
                    Ok(WasmTy::F64)
                }
                NumericMode::I64 => {
                    self.function.instructions().i64_const(*n as i64);
                    Ok(WasmTy::I64)
                }
            },
            Expr::Bool(b) => {
                self.function
                    .instructions()
                    .i32_const(if *b { 1 } else { 0 });
                Ok(WasmTy::I32)
            }
            Expr::Ident(name) => {
                if let Some((idx, ty)) = self.lookup(name) {
                    self.function.instructions().local_get(idx);
                    return Ok(ty);
                }
                Err(BuildError::NotYetImplemented(format!(
                    "ident `{name}` — AOT only resolves fn params today"
                )))
            }
            Expr::BinOp { op, left, right } => {
                let lt = self.lower_expr(c, *left)?;
                let rt = self.lower_expr(c, *right)?;
                if lt != rt {
                    return Err(BuildError::Real(format!(
                        "binop type mismatch slipped past check: {lt:?} vs {rt:?}"
                    )));
                }
                let mut s = self.function.instructions();
                match (lt, op) {
                    // f64 arithmetic + comparison
                    (WasmTy::F64, BinOp::Add) => {
                        s.f64_add();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Sub) => {
                        s.f64_sub();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Mul) => {
                        s.f64_mul();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Div) => {
                        s.f64_div();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Mod) => Err(BuildError::NotYetImplemented(
                        "`%` on f64 — AOT only supports `%` in integer-narrowed mode today (would need libm fmod)".into(),
                    )),
                    (WasmTy::F64, BinOp::Lt) => {
                        s.f64_lt();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Gt) => {
                        s.f64_gt();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Le) => {
                        s.f64_le();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Ge) => {
                        s.f64_ge();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Eq) => {
                        s.f64_eq();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Neq) => {
                        s.f64_ne();
                        Ok(WasmTy::I32)
                    }
                    // i64 arithmetic + comparison (type-narrowed integer mode)
                    (WasmTy::I64, BinOp::Add) => {
                        s.i64_add();
                        Ok(WasmTy::I64)
                    }
                    (WasmTy::I64, BinOp::Sub) => {
                        s.i64_sub();
                        Ok(WasmTy::I64)
                    }
                    (WasmTy::I64, BinOp::Mul) => {
                        s.i64_mul();
                        Ok(WasmTy::I64)
                    }
                    (WasmTy::I64, BinOp::Mod) => {
                        s.i64_rem_s();
                        Ok(WasmTy::I64)
                    }
                    (WasmTy::I64, BinOp::BitAnd) => { s.i64_and(); Ok(WasmTy::I64) }
                    (WasmTy::I64, BinOp::BitOr)  => { s.i64_or();  Ok(WasmTy::I64) }
                    (WasmTy::I64, BinOp::BitXor) => { s.i64_xor(); Ok(WasmTy::I64) }
                    (WasmTy::I64, BinOp::Shl)    => { s.i64_shl(); Ok(WasmTy::I64) }
                    (WasmTy::I64, BinOp::Shr)    => { s.i64_shr_s(); Ok(WasmTy::I64) }
                    (WasmTy::I64, BinOp::Lt) => {
                        s.i64_lt_s();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I64, BinOp::Gt) => {
                        s.i64_gt_s();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I64, BinOp::Le) => {
                        s.i64_le_s();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I64, BinOp::Ge) => {
                        s.i64_ge_s();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I64, BinOp::Eq) => {
                        s.i64_eq();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I64, BinOp::Neq) => {
                        s.i64_ne();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I32, BinOp::Eq) => {
                        s.i32_eq();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I32, BinOp::Neq) => {
                        s.i32_ne();
                        Ok(WasmTy::I32)
                    }
                    (other, op) => Err(BuildError::NotYetImplemented(format!(
                        "binop {op:?} on {other:?} — AOT covers number ops + boolean ===/!=="
                    ))),
                }
            }
            Expr::Call { callee, args } => self.lower_call(c, *callee, args),
            Expr::Member { .. } => Err(BuildError::NotYetImplemented(
                "bare member access in expression — AOT only handles `console.log`".into(),
            )),
            Expr::String(_) => Err(BuildError::NotYetImplemented(
                "string in non-`console.log` position — AOT defers strings (P3.4)".into(),
            )),
            Expr::Assign { target, value } => {
                let target_eid = *target;
                let value_eid = *value;
                let Expr::Ident(tname_ref) = c.ast.get_expr(target_eid) else {
                    return Err(BuildError::NotYetImplemented(
                        "assignment to non-ident target".into(),
                    ));
                };
                let tname = tname_ref.clone();
                let (slot, ty) = self.lookup(&tname).ok_or_else(|| {
                    BuildError::Real(format!("assignment to undeclared `{tname}`"))
                })?;
                let vt = self.lower_expr(c, value_eid)?;
                if vt != ty {
                    return Err(BuildError::Real(format!(
                        "assignment type mismatch on `{tname}`: target {ty:?}, value {vt:?}"
                    )));
                }
                // Use local.tee: stores top of stack into local AND leaves
                // the value on the stack — that's the assignment expression's
                // result, which subsequent ops (or the surrounding stmt's
                // drop) consumes.
                self.function.instructions().local_tee(slot);
                Ok(ty)
            }
            Expr::Index { .. } => Err(BuildError::NotYetImplemented(
                "indexing — AOT defers strings/arrays".into(),
            )),
            Expr::Array(_) => Err(BuildError::NotYetImplemented(
                "array literal — AOT defers".into(),
            )),
            Expr::ArrowFn { .. } => Err(BuildError::NotYetImplemented(
                "arrow fn — AOT only handles top-level FnDecl today".into(),
            )),
        }
    }

    fn lower_call(
        &mut self,
        c: &mut Compiler,
        callee: ExprId,
        args: &[ExprId],
    ) -> Result<WasmTy, BuildError> {
        // Special-case console.log: dispatch on arg type/shape.
        if let Expr::Member { obj, name } = c.ast.get_expr(callee)
            && let Expr::Ident(obj_name) = c.ast.get_expr(*obj)
            && obj_name == "console"
            && name == "log"
        {
            if args.len() != 1 {
                return Err(BuildError::NotYetImplemented(format!(
                    "console.log with {} args — AOT supports exactly 1 arg today",
                    args.len()
                )));
            }
            // If the arg is a literal string, route to print_static.
            if let Expr::String(s) = c.ast.get_expr(args[0]) {
                let s = s.clone();
                let (ptr, len) = c.intern_string(&s);
                let mut sink = self.function.instructions();
                sink.i32_const(ptr as i32);
                sink.i32_const(len as i32);
                sink.call(FN_PRINT_STATIC);
                return Ok(WasmTy::Void);
            }
            // Otherwise lower the expression and dispatch on its wasm type.
            let arg_ty = self.lower_expr(c, args[0])?;
            match arg_ty {
                WasmTy::F64 | WasmTy::I64 => {
                    self.function.instructions().call(FN_PRINT_I64);
                    Ok(WasmTy::Void)
                }
                _ => Err(BuildError::NotYetImplemented(format!(
                    "console.log of wasm type {arg_ty:?} — AOT only prints numbers + literal strings today"
                ))),
            }
        } else if let Expr::Ident(fname_ref) = c.ast.get_expr(callee) {
            let fname = fname_ref.clone();
            // User fn call.
            let user = c.user_fns.get(&fname).cloned().ok_or_else(|| {
                BuildError::NotYetImplemented(format!(
                    "calling `{fname}` — AOT only resolves top-level FnDecls"
                ))
            })?;
            if args.len() != user.params.len() {
                return Err(BuildError::Real(format!(
                    "arity mismatch calling `{fname}`: expected {}, got {}",
                    user.params.len(),
                    args.len()
                )));
            }
            for (a, (_, expected)) in args.iter().zip(user.params.iter()) {
                let ty = self.lower_expr(c, *a)?;
                if ty != *expected {
                    return Err(BuildError::Real(format!(
                        "arg type mismatch for `{fname}`: expected {expected:?}, got {ty:?}"
                    )));
                }
            }
            self.function.instructions().call(user.index);
            Ok(user.ret)
        } else {
            Err(BuildError::NotYetImplemented(
                "callee shape — AOT only handles `console.log` and direct named-fn calls".into(),
            ))
        }
    }
}

impl UserFn {
    fn clone(&self) -> UserFn {
        UserFn {
            name: self.name.clone(),
            params: self.params.clone(),
            ret: self.ret,
            index: self.index,
            type_index: self.type_index,
        }
    }
}

impl Clone for UserFn {
    fn clone(&self) -> Self {
        UserFn::clone(self)
    }
}

/// Walk the AST and decide whether the whole module can be lowered with
/// `number = i64`. Returns `I64` only if there is no `/` operator and every
/// numeric literal is integer-valued (and finite). Otherwise `F64`.
fn detect_numeric_mode(ast: &Ast) -> NumericMode {
    fn pure_stmt(ast: &Ast, s: &Stmt) -> bool {
        match s {
            Stmt::Expr(e) => pure_expr(ast, *e),
            Stmt::Return(maybe) => maybe.is_none_or(|e| pure_expr(ast, e)),
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                pure_expr(ast, *cond)
                    && pure_stmt(ast, then_branch)
                    && else_branch.as_ref().is_none_or(|b| pure_stmt(ast, b))
            }
            Stmt::While { cond, body } => pure_expr(ast, *cond) && pure_stmt(ast, body),
            Stmt::Block(ss) => ss.iter().all(|s| pure_stmt(ast, s)),
            Stmt::FnDecl { body, .. } => body.iter().all(|s| pure_stmt(ast, s)),
            Stmt::LetDecl { init, .. } => pure_expr(ast, *init),
        }
    }
    fn pure_expr(ast: &Ast, eid: ExprId) -> bool {
        match ast.get_expr(eid) {
            Expr::Number(n) => n.is_finite() && n.fract() == 0.0,
            Expr::Bool(_) | Expr::Ident(_) | Expr::String(_) => true,
            Expr::BinOp { op, left, right } => {
                // `/` keeps us in F64 mode (integer div would discard remainder
                // — JS spec says number/number → number, never integer).
                // Bit ops + `%` are integer-only and stay in I64 mode.
                !matches!(op, BinOp::Div) && pure_expr(ast, *left) && pure_expr(ast, *right)
            }
            Expr::Call { callee, args } => {
                pure_expr(ast, *callee) && args.iter().all(|a| pure_expr(ast, *a))
            }
            Expr::Member { obj, .. } => pure_expr(ast, *obj),
            Expr::Assign { value, .. } => pure_expr(ast, *value),
            Expr::Index { obj, index } => pure_expr(ast, *obj) && pure_expr(ast, *index),
            Expr::Array(els) => els.iter().all(|e| pure_expr(ast, *e)),
            Expr::ArrowFn { body, .. } => body.iter().all(|s| pure_stmt(ast, s)),
        }
    }
    if ast.stmts.iter().all(|s| pure_stmt(ast, s)) {
        NumericMode::I64
    } else {
        NumericMode::F64
    }
}

/// Helper `(func $print_i64 (param $n <f64|i64>))`.
///
/// In F64 mode: param is f64, truncated (saturating) to i64 first.
/// In I64 mode: param is i64 directly — the trunc step is skipped.
/// Then writes ASCII decimal digits + newline to fd 1 via fd_write.
fn emit_print_i64(mode: NumericMode) -> Function {
    // In I64 mode the param is already i64, so $i isn't a separate slot —
    // we still keep one for symmetry (and to avoid renumbering everything).
    let locals = vec![
        (1, ValType::I64), // $i
        (1, ValType::I32), // $p
        (1, ValType::I32), // $digit
    ];
    let mut f = Function::new(locals);
    let n_param: u32 = 0;
    let i_local: u32 = 1;
    let p_local: u32 = 2;
    let d_local: u32 = 3;
    let mut s = f.instructions();

    // newline at byte 39
    s.i32_const(MEM_DIGIT_BUF_END as i32);
    s.i32_const(b'\n' as i32);
    s.i32_store8(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    });

    // i = (mode==f64 ? trunc_sat_f64_s(n) : n)
    s.local_get(n_param);
    if matches!(mode, NumericMode::F64) {
        s.i64_trunc_sat_f64_s();
    }
    s.local_set(i_local);

    // p = 38 (write digits backward starting here)
    s.i32_const((MEM_DIGIT_BUF_END - 1) as i32);
    s.local_set(p_local);

    // if (i == 0) write '0', dec p, skip loop
    s.local_get(i_local);
    s.i64_eqz();
    s.if_(BlockType::Empty);
    {
        s.local_get(p_local);
        s.i32_const(b'0' as i32);
        s.i32_store8(MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        });
        s.local_get(p_local);
        s.i32_const(1);
        s.i32_sub();
        s.local_set(p_local);
    }
    s.else_();
    {
        // loop: write digits
        s.loop_(BlockType::Empty);
        {
            // digit = (i % 10) as i32
            s.local_get(i_local);
            s.i64_const(10);
            s.i64_rem_s();
            s.i32_wrap_i64();
            s.local_set(d_local);

            // *p = '0' + digit
            s.local_get(p_local);
            s.local_get(d_local);
            s.i32_const(b'0' as i32);
            s.i32_add();
            s.i32_store8(MemArg {
                offset: 0,
                align: 0,
                memory_index: 0,
            });

            // p -= 1
            s.local_get(p_local);
            s.i32_const(1);
            s.i32_sub();
            s.local_set(p_local);

            // i /= 10
            s.local_get(i_local);
            s.i64_const(10);
            s.i64_div_s();
            s.local_set(i_local);

            // br_if loop while i != 0 (we only handle non-negative for now;
            // signed div on a negative would loop forever — caller must
            // pass a non-negative number).
            s.local_get(i_local);
            s.i64_const(0);
            s.i64_gt_s();
            s.br_if(0);
        }
        s.end();
    }
    s.end();

    // build iovec: ptr = p+1, len = (MEM_DIGIT_BUF_END+1) - (p+1) = MEM_DIGIT_BUF_END - p
    s.i32_const(MEM_IOVEC as i32);
    s.local_get(p_local);
    s.i32_const(1);
    s.i32_add();
    s.i32_store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    });

    s.i32_const((MEM_IOVEC + 4) as i32);
    s.i32_const(MEM_DIGIT_BUF_END as i32);
    s.local_get(p_local);
    s.i32_sub();
    s.i32_store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    });

    // call fd_write(1, MEM_IOVEC, 1, MEM_NWRITTEN)
    s.i32_const(1);
    s.i32_const(MEM_IOVEC as i32);
    s.i32_const(1);
    s.i32_const(MEM_NWRITTEN as i32);
    s.call(FN_FD_WRITE);
    s.drop();

    s.end();
    f
}

/// Helper `(func $print_static (param $ptr i32) (param $len i32))`.
/// Writes `len` bytes starting at `ptr` via fd_write.
fn emit_print_static() -> Function {
    let mut f = Function::new(vec![]);
    let ptr_param: u32 = 0;
    let len_param: u32 = 1;
    let mut s = f.instructions();

    // iovec at MEM_IOVEC: { ptr, len }
    s.i32_const(MEM_IOVEC as i32);
    s.local_get(ptr_param);
    s.i32_store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    });

    s.i32_const((MEM_IOVEC + 4) as i32);
    s.local_get(len_param);
    s.i32_store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    });

    // fd_write(1, MEM_IOVEC, 1, MEM_NWRITTEN)
    s.i32_const(1);
    s.i32_const(MEM_IOVEC as i32);
    s.i32_const(1);
    s.i32_const(MEM_NWRITTEN as i32);
    s.call(FN_FD_WRITE);
    s.drop();

    s.end();
    f
}
