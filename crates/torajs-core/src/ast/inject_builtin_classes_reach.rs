//! Can this program OBSERVE a native error's instance shape? — the
//! reachability gate for the runtime-thrown force-inject (RFC
//! 20260825-injection-reachability 刀 B).
//!
//! The injected Error hierarchy is what a runtime raise builds its
//! catchable instance from. A program that can never LOOK at that
//! instance doesn't need the classes: the raise degrades to the
//! named bare-string fallback (`torajs-throw::throw_native_str`,
//! `"<Name>: <msg>"` since 刀 A), whose uncaught first line renders
//! the same. Observation happens through exactly three doors, each
//! answered conservatively from the AST arena (any hit = inject,
//! exactly today's behavior — the gate can only ever SKIP for
//! programs where all three doors are provably closed):
//!
//! 1. **catch face** — a `try`/`catch` receives the thrown value; a
//!    user `throw` implies error-flow the program is exercising on
//!    purpose. Any `Stmt::Try` / `Stmt::Throw` in the arena hits.
//! 2. **rejection face** — in async/promise context a runtime raise
//!    becomes a rejection value: a handler (`then`/`catch`/
//!    `finally`) or the unhandled-rejection reporter reads it as an
//!    instance. Any async fn, `Promise` / `queueMicrotask` name, or
//!    a `then`/`catch`/`finally` member hits.
//! 3. **name face** — the existing `referenced` gate (bare Ident /
//!    `new` / member / `catch (e: T)` annotation) already implies
//!    injection per class; the caller ORs it in.
//!
//! Dynamic-dispatch escapes (`x[k]()`, `.call`/`.apply`/`.bind`)
//! need no door of their own: a dynamic call can only OBSERVE an
//! instance it was handed, and every handing-over path is one of
//! the three doors above (catch binding, rejection handler, or a
//! named Error the program constructed itself).

use super::ast_def::Ast;
use crate::ast::Expr;

/// True when any of the observation doors is open — the caller
/// keeps today's force-inject. False ⇔ a native raise in this
/// program can only ever surface through the uncaught reporter,
/// whose bare-string rendering 刀 A made instance-identical.
pub(crate) fn instance_shape_observable(ast: &Ast) -> bool {
    // Door 1 — catch face. The parser-recorded flag, NOT an
    // `ast.stmts` scan: that vec holds only the top level (statement
    // bodies are inline, not arena-flat), so a scan misses a
    // try/catch inside a function body — which is where almost every
    // real one lives (the first gate build failed 181 conformance
    // cases exactly this way).
    if ast.has_try_or_throw {
        return true;
    }
    // Door 2 — rejection face. `await` itself leaves no AST node
    // (the L.2 desugar reads `.value`), but a promise VALUE cannot
    // appear out of thin air: its producers are an async fn, the
    // `Promise` ctor/statics, a promise-answering host global
    // (`fetch` / `Bun`), a dynamic `import()`, or a thenable the
    // program spelled itself — and each of those is a name this
    // door watches.
    if !ast.async_fns.is_empty() || ast.dyn_import_present {
        return true;
    }
    ast.exprs.iter().any(|e| {
        matches!(e, Expr::Ident(n)
            if n == "Promise" || n == "queueMicrotask" || n == "fetch" || n == "Bun")
            || matches!(e, Expr::Member { name, .. }
                if name == "then" || name == "catch" || name == "finally")
    })
}
