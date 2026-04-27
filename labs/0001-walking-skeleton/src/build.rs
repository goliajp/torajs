//! AOT to wasm — P3.1 stub.
//!
//! This is intentionally minimal: it handles exactly one shape of program,
//! namely `console.log("<string literal>")` as a single top-level statement.
//! Anything else returns a `Err`. P3.2/3.3/... extend this to real
//! type-directed lowering.
//!
//! Output is a tiny WASI module:
//!
//! - imports `wasi_snapshot_preview1.fd_write`
//! - exports a 1-page linear memory and a `_start` function
//! - bakes the string + iovec + nwritten slot into the data section
//! - `_start` calls `fd_write(stdout=1, &iovec, 1, &nwritten)` and `drop`s the errno

use std::fs;
use std::path::Path;

use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, MemorySection, MemoryType, Module, TypeSection, ValType,
};

use crate::ast::{Ast, Expr, Stmt};

const WASM_PAGE_BYTES: u32 = 65536;

/// AOT build outcome. The two error variants exit tr with different codes so
/// the bench harness can distinguish "torajs-aot doesn't support this program
/// shape yet" (skip) from "the build pipeline broke" (fail).
#[derive(Debug)]
pub enum BuildError {
    /// The program is well-formed but uses features the current AOT pass
    /// hasn't grown yet (P3.1 only handles `console.log("<literal>")`).
    NotYetImplemented(String),
    /// A real bug — i/o error, codegen invariant violated, etc.
    Real(String),
}

pub fn build(ast: &Ast, out_path: &Path) -> Result<(), BuildError> {
    let mut text = extract_single_console_log_string(ast).map_err(BuildError::NotYetImplemented)?;
    // `console.log` always tacks on a newline.
    text.push('\n');
    let bytes = text.as_bytes();
    let str_len = bytes.len() as u32;

    // Layout in memory page 0:
    //   [0..str_len)            — the string itself
    //   [iovec_off..iovec_off+8) — iovec { buf_ptr=0, buf_len=str_len }, 4-byte aligned
    //   [nwritten_off..+4)      — the i32 fd_write writes the byte count into
    let iovec_off = (str_len + 3) & !3; // align up to 4 bytes
    let nwritten_off = iovec_off + 8;
    if nwritten_off + 4 > WASM_PAGE_BYTES {
        return Err(BuildError::NotYetImplemented(format!(
            "string too large for one wasm page (need {} bytes, page is {}); multi-page memory is P3.5",
            nwritten_off + 4,
            WASM_PAGE_BYTES
        )));
    }

    let mut module = Module::new();

    // .types — index 0: fd_write signature, index 1: _start signature
    let mut types = TypeSection::new();
    types.ty().function(
        [ValType::I32, ValType::I32, ValType::I32, ValType::I32],
        [ValType::I32],
    );
    types.ty().function([], []);
    module.section(&types);

    // .import — wasi_snapshot_preview1.fd_write : type 0
    let mut imports = ImportSection::new();
    imports.import(
        "wasi_snapshot_preview1",
        "fd_write",
        EntityType::Function(0),
    );
    module.section(&imports);

    // .function — _start as type 1 (function index 1, since import takes index 0)
    let mut functions = FunctionSection::new();
    functions.function(1);
    module.section(&functions);

    // .memory — 1 page (64 KiB)
    let mut memories = MemorySection::new();
    memories.memory(MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);

    // .export — memory + _start
    let mut exports = ExportSection::new();
    exports.export("memory", ExportKind::Memory, 0);
    exports.export("_start", ExportKind::Func, 1);
    module.section(&exports);

    // .code — _start body
    let mut code = CodeSection::new();
    let mut start_fn = Function::new([]);
    {
        let mut sink = start_fn.instructions();
        sink.i32_const(1); // stdout
        sink.i32_const(iovec_off as i32); // iovec ptr
        sink.i32_const(1); // num iovecs
        sink.i32_const(nwritten_off as i32); // nwritten ptr
        sink.call(0); // call fd_write (import = function index 0)
        sink.drop(); // discard errno
        sink.end();
    }
    code.function(&start_fn);
    module.section(&code);

    // .data — string bytes + iovec
    let mut data = DataSection::new();
    data.active(0, &ConstExpr::i32_const(0), bytes.iter().copied());
    let mut iovec = Vec::with_capacity(8);
    iovec.extend_from_slice(&0u32.to_le_bytes());
    iovec.extend_from_slice(&str_len.to_le_bytes());
    data.active(0, &ConstExpr::i32_const(iovec_off as i32), iovec);
    module.section(&data);

    let wasm = module.finish();
    fs::write(out_path, &wasm)
        .map_err(|e| BuildError::Real(format!("writing {}: {e}", out_path.display())))?;
    Ok(())
}

fn extract_single_console_log_string(ast: &Ast) -> Result<String, String> {
    if ast.stmts.len() != 1 {
        return Err(format!(
            "P3.1 AOT only supports a single `console.log(\"<string>\")` statement (got {} statements)",
            ast.stmts.len()
        ));
    }
    let Stmt::Expr(eid) = &ast.stmts[0] else {
        return Err("P3.1 AOT only supports a single expression statement".into());
    };
    let Expr::Call { callee, args } = ast.get_expr(*eid) else {
        return Err("P3.1 AOT only supports a `console.log(...)` call".into());
    };
    if args.len() != 1 {
        return Err(format!(
            "P3.1 AOT requires console.log with exactly one argument (got {})",
            args.len()
        ));
    }
    let Expr::Member { obj, name } = ast.get_expr(*callee) else {
        return Err("P3.1 AOT only supports `console.log`".into());
    };
    let Expr::Ident(obj_name) = ast.get_expr(*obj) else {
        return Err("P3.1 AOT only supports `console.log`".into());
    };
    if obj_name != "console" || name != "log" {
        return Err(format!(
            "P3.1 AOT only supports `console.log` (got `{obj_name}.{name}`)"
        ));
    }
    let Expr::String(s) = ast.get_expr(args[0]) else {
        return Err("P3.1 AOT requires a literal string argument to console.log".into());
    };
    Ok(s.clone())
}
