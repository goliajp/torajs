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

#[derive(Debug, Clone, Copy, PartialEq)]
enum WasmTy {
    F64,
    I32,
    Void,
}

impl WasmTy {
    fn from_ann(ann: &str) -> Option<WasmTy> {
        match ann {
            "number" => Some(WasmTy::F64),
            "boolean" => Some(WasmTy::I32),
            "void" => Some(WasmTy::Void),
            _ => None,
        }
    }

    fn val_type(self) -> Option<ValType> {
        match self {
            WasmTy::F64 => Some(ValType::F64),
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
}

impl<'a> Compiler<'a> {
    fn new(ast: &'a Ast) -> Self {
        Self {
            ast,
            module: Module::new(),
            static_strings: Vec::new(),
            static_cursor: MEM_STATIC_START,
            user_fns: HashMap::new(),
            user_fn_order: Vec::new(),
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
                    let ty = WasmTy::from_ann(ann).ok_or_else(|| {
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
                    Some(ann) => WasmTy::from_ann(ann).ok_or_else(|| {
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
        types.ty().function([ValType::F64], []);
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
        code.function(&emit_print_i64());
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
        let mut fb = FnBuilder::new(info.clone());
        for (i, (pname, ty)) in info.params.iter().enumerate() {
            fb.add_param(pname.clone(), *ty, i as u32);
        }
        for s in &body_clone {
            fb.lower_stmt(self, s)?;
        }
        fb.function.instructions().end();
        Ok(fb.into_function())
    }

    fn compile_main(&mut self) -> Result<Function, BuildError> {
        let info = SignatureRef {
            params: Vec::new(),
            ret: WasmTy::Void,
        };
        let mut fb = FnBuilder::new(info);
        let stmts: Vec<Stmt> = self
            .ast
            .stmts
            .iter()
            .filter(|s| !matches!(s, Stmt::FnDecl { .. }))
            .cloned()
            .collect();
        for s in &stmts {
            fb.lower_stmt(self, s)?;
        }
        fb.function.instructions().end();
        Ok(fb.into_function())
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
    fn new(sig: SignatureRef) -> Self {
        Self {
            sig,
            function: Function::new([]),
            scopes: vec![HashMap::new()],
        }
    }

    fn add_param(&mut self, name: String, ty: WasmTy, idx: u32) {
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
            Stmt::LetDecl { name, .. } => Err(BuildError::NotYetImplemented(format!(
                "`let`/`const` binding `{name}` inside fn body — AOT P3.3 only supports fn parameters as locals (P3.3+ adds let)"
            ))),
            Stmt::While { .. } => Err(BuildError::NotYetImplemented(
                "`while` — AOT will add this in a follow-up step".into(),
            )),
        }
    }

    fn lower_expr(&mut self, c: &mut Compiler, eid: ExprId) -> Result<WasmTy, BuildError> {
        match c.ast.get_expr(eid) {
            Expr::Number(n) => {
                self.function.instructions().f64_const((*n).into());
                Ok(WasmTy::F64)
            }
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
                match (lt, op) {
                    (WasmTy::F64, BinOp::Add) => {
                        self.function.instructions().f64_add();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Sub) => {
                        self.function.instructions().f64_sub();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Mul) => {
                        self.function.instructions().f64_mul();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Div) => {
                        self.function.instructions().f64_div();
                        Ok(WasmTy::F64)
                    }
                    (WasmTy::F64, BinOp::Lt) => {
                        self.function.instructions().f64_lt();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Gt) => {
                        self.function.instructions().f64_gt();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Le) => {
                        self.function.instructions().f64_le();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Ge) => {
                        self.function.instructions().f64_ge();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Eq) => {
                        self.function.instructions().f64_eq();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::F64, BinOp::Neq) => {
                        self.function.instructions().f64_ne();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I32, BinOp::Eq) => {
                        self.function.instructions().i32_eq();
                        Ok(WasmTy::I32)
                    }
                    (WasmTy::I32, BinOp::Neq) => {
                        self.function.instructions().i32_ne();
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
            Expr::Assign { .. } => Err(BuildError::NotYetImplemented(
                "assignment expression — AOT will add when locals land".into(),
            )),
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
                WasmTy::F64 => {
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

/// Helper `(func $print_i64 (param $n f64))`. Truncates to i64 (saturating)
/// and writes ASCII decimal digits + newline to fd 1 via fd_write.
fn emit_print_i64() -> Function {
    let locals = vec![
        (1, ValType::I64), // $i — the integer being formatted
        (1, ValType::I32), // $p — current write pointer
        (1, ValType::I32), // $digit — temp
    ];
    let mut f = Function::new(locals);
    let n_param: u32 = 0; // f64
    let i_local: u32 = 1; // i64
    let p_local: u32 = 2; // i32
    let d_local: u32 = 3; // i32
    let mut s = f.instructions();

    // newline at byte 39
    s.i32_const(MEM_DIGIT_BUF_END as i32);
    s.i32_const(b'\n' as i32);
    s.i32_store8(MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    });

    // i = trunc_sat_f64_s(n)
    s.local_get(n_param);
    s.i64_trunc_sat_f64_s();
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
