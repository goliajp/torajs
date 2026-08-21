//! I/O-flavored `Type::Object("NAMESPACE")` static-namespace
//! arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 201 — eleventh
//! sub-batch of check_type_of_member.rs per-type-family
//! decomposition; mirrors chunks 191-200 try_match shape).
//!
//! Covers the I/O-shaped namespace arms (sibling to chunk
//! 200's compute/math/JSON/etc namespaces):
//! - `Date` static (now / parse / UTC)
//! - `Bun` (write / file / gc)
//! - `BunFile` (text / exists / json / size)
//! - `Response` (text / status)
//! - `process` (exit / cwd / platform / argv / env /
//!   stdout / stderr)
//! - `Bun.argv` (folded into the process.argv union)
//! - `env` catch-all (`Object("env"), _`)
//! - `process_stdout` / `process_stderr` (write)
//! - `fs` sync (readFileSync / writeFileSync /
//!   appendFileSync / unlinkSync / mkdirSync /
//!   existsSync / readdirSync)
//! - `fs/promises` (readdir / readFile / writeFile /
//!   appendFile / unlink / mkdir / exists)
//! - `String` static (fromCharCode / fromCodePoint)
//!
//! Returns `Some(Ok(_))` on hit, `None` when `(obj_ty, name)`
//! doesn't match. Sibling to
//! [`crate::check_type_of_member_namespace`] (chunk 200) —
//! the two cover disjoint namespace tags and run sequentially
//! at the pre-match dispatch site.

use crate::check::Type;

pub(crate) fn try_match(obj_ty: &Type, name: &str) -> Option<Result<Type, String>> {
    let ty = match (obj_ty, name) {
        // Date.now() — static, returns ms-since-epoch.
        (Type::Object("Date"), "now") => Type::Function(Vec::new(), Box::new(Type::Number)),
        // Phase 2.0b.2 — Date.parse(s) returns ms-since-epoch
        // (or NaN sentinel — tr returns INT64_MIN; spec is NaN).
        (Type::Object("Date"), "parse") => {
            Type::Function(vec![Type::String], Box::new(Type::Number))
        }
        // Date.UTC(year, month, day, hour, min, sec, ms) — UTC
        // interpretation; returns ms-since-epoch. Min 2 args.
        // tr accepts the 7-arg form via the same dispatch path
        // as `new Date(...)` component ctor; missing trailing
        // args default to month=0, day=1, rest=0 — but that
        // padding happens at desugar time, which doesn't
        // intercept `Date.UTC(...)` (only `new Date(...)`).
        // For Phase 2.0b.2, tr's Date.UTC requires explicit
        // 7 args; arity-aware desugar comes in 2.0c.
        (Type::Object("Date"), "UTC") => {
            Type::Function(vec![Type::Number; 7], Box::new(Type::Number))
        }
        /* v0.3 #2 — Bun namespace (minimum).
         * Bun.write(path, data) — bun-shape file write,
         * routes to the same fs intrinsic. Bun.file(path)
         * (chained-method shape returning a File object)
         * lands when the surface gains object-result Calls. */
        (Type::Object("Bun"), "write") => {
            Type::Function(vec![Type::String, Type::String], Box::new(Type::Void))
        }
        /* T-19 (v0.5.0) — `Bun.file(path)` returns an
         * opaque BunFile handle. The user calls `.text()`
         * (or future `.json()` / `.arrayBuffer()`) on it
         * to actually read. The handle is internally
         * `Type::String` (just the path) since the
         * methods all dispatch through fs.readFileSync.
         * Type::Object("BunFile") sentinel keeps the
         * methods scoped so plain Strings don't match. */
        (Type::Object("Bun"), "file") => {
            Type::Function(vec![Type::String], Box::new(Type::Object("BunFile")))
        }
        /* V3-08 — `Bun.gc(synchronous)`. tora's Bacon-Rajan
         * cycle collector triggers regardless of the bool
         * arg (we ignore it; bun uses it to gate JSC's
         * concurrent GC). Both runtimes return void. */
        (Type::Object("Bun"), "gc") => Type::Function(vec![Type::Boolean], Box::new(Type::Void)),
        (Type::Object("BunFile"), "text") => {
            Type::Function(Vec::new(), Box::new(Type::Promise(Box::new(Type::String))))
        }
        /* T-19.c (v0.5.0) — `Bun.file(p).exists()`. Bun
         * exposes this as a fast existence-probe that
         * doesn't open the file. Maps to fs.existsSync
         * in the MVP "synchronous-then-resolve" model. */
        (Type::Object("BunFile"), "exists") => {
            Type::Function(Vec::new(), Box::new(Type::Promise(Box::new(Type::Boolean))))
        }
        /* T-19.d (v0.5.0) — `Bun.file(p).json()` returns
         * Promise<Any>. The actual return type comes from
         * the caller-driven `let X: T = await Bun.file(p)
         * .json()` shape detection in ssa_lower's LetDecl
         * arm — JSON.parse drives parsing per the slot's
         * concrete T (number / string / Struct / Array<T>
         * / etc.). At the typecheck layer we accept any
         * slot type as long as the JSON parser knows how
         * to handle it; concrete validation happens at
         * lower time. */
        (Type::Object("BunFile"), "json") => {
            Type::Function(Vec::new(), Box::new(Type::Promise(Box::new(Type::Any))))
        }
        /* T-18.c (v0.5.0) — `Bun.file(p).size` synchronous
         * property (NOT a method). Returns the file's
         * byte size, or -1 if the path is missing or
         * non-regular (bun returns 0 for missing — tr
         * uses -1 to keep the missing case observable
         * until typed-throw fs lands). */
        (Type::Object("BunFile"), "size") => Type::Number,
        /* T-21 (v0.6.0) — `fetch(url)` Response surface.
         * `.text()` returns the (already-loaded) body as
         * `Promise<string>`; `.status` is the HTTP status
         * code (0 on transport error). `.ok` and JSON
         * parsing land alongside the fetch options
         * follow-up. */
        (Type::Object("Response"), "text") => {
            Type::Function(Vec::new(), Box::new(Type::Promise(Box::new(Type::String))))
        }
        (Type::Object("Response"), "status") => Type::Number,
        /* v0.3 #3 — process surface (minimum). */
        (Type::Object("process"), "exit") => {
            Type::Function(vec![Type::Number], Box::new(Type::Void))
        }
        (Type::Object("process"), "cwd") => Type::Function(Vec::new(), Box::new(Type::String)),
        /* `process.platform` — value access, not a Call.
         * Returned as Type::String; ssa_lower's Member arm
         * emits a runtime call to __torajs_process_platform. */
        (Type::Object("process"), "platform") => Type::String,
        /* `process.argv` / `Bun.argv` — runtime array of
         * argv strings. Lowered by ssa_lower's Member arm
         * to __torajs_process_argv(). */
        (Type::Object("process") | Type::Object("Bun"), "argv") => {
            Type::Array(Box::new(Type::String))
        }
        /* `process.env` — env-namespace Object; member
         * access on it (`process.env.NAME`) routes through
         * the (Object("env"), _) arm below to runtime getenv. */
        (Type::Object("process"), "env") => Type::Object("env"),
        /* `process.env.NAME` — Nullable<String> (NULL when
         * var unset; tr's undefined→null bridge keeps
         * `=== undefined` round-tripping). */
        (Type::Object("env"), _) => Type::Nullable(Box::new(Type::String)),
        /* T-03 (v0.3.0) — process.{stdout, stderr, stdin}
         * value-Member: each exposes its own Object so the
         * downstream `.write` / `.read` Call resolves at
         * the (Object("process_stdout"), "write") arm
         * below. */
        (Type::Object("process"), "stdout") => Type::Object("process_stdout"),
        (Type::Object("process"), "stderr") => Type::Object("process_stderr"),
        /* T-03 — process.stdout / process.stderr.write(s)
         * Call shape. Returns Boolean to match bun's
         * `process.stdout.write(s)` signature (true on
         * success, false on backpressure / error — tr
         * panics on short write so it always returns true
         * when control returns). */
        (Type::Object("process_stdout") | Type::Object("process_stderr"), "write") => {
            Type::Function(vec![Type::String], Box::new(Type::Boolean))
        }
        /* v0.3 #1 — fs module surface (Phase 2.0a substrate).
         * Synchronous file I/O; throw on error is Phase 2.0b. */
        (Type::Object("fs"), "readFileSync") => {
            Type::Function(vec![Type::String], Box::new(Type::String))
        }
        (Type::Object("fs"), "writeFileSync" | "appendFileSync") => {
            Type::Function(vec![Type::String, Type::String], Box::new(Type::Void))
        }
        (Type::Object("fs"), "unlinkSync" | "mkdirSync") => {
            Type::Function(vec![Type::String], Box::new(Type::Void))
        }
        (Type::Object("fs"), "existsSync") => {
            Type::Function(vec![Type::String], Box::new(Type::Boolean))
        }
        /* T-18.b (v0.5.0) — fs.readdirSync(path) returns
         * Array<string> with one entry per child (`.` /
         * `..` filtered, matching bun spec). */
        (Type::Object("fs"), "readdirSync") => Type::Function(
            vec![Type::String],
            Box::new(Type::Array(Box::new(Type::String))),
        ),
        (Type::Object("fs_promises"), "readdir") => Type::Function(
            vec![Type::String],
            Box::new(Type::Promise(Box::new(Type::Array(Box::new(Type::String))))),
        ),
        /* T-18.a (v0.5.0) — `fs/promises` module. Each
         * method calls the matching sync helper from
         * `fs.<X>Sync` then wraps the result in
         * Promise.resolve(...). MVP "synchronous-then-
         * resolve" — real I/O suspension needs T-16
         * state-machine async/await. Bun-parity:
         * `import { readFile } from "fs/promises"; await
         * readFile(p)` yields the file contents
         * byte-identical with bun. */
        (Type::Object("fs_promises"), "readFile") => Type::Function(
            vec![Type::String],
            Box::new(Type::Promise(Box::new(Type::String))),
        ),
        (Type::Object("fs_promises"), "writeFile" | "appendFile") => Type::Function(
            vec![Type::String, Type::String],
            Box::new(Type::Promise(Box::new(Type::Void))),
        ),
        (Type::Object("fs_promises"), "unlink" | "mkdir") => Type::Function(
            vec![Type::String],
            Box::new(Type::Promise(Box::new(Type::Void))),
        ),
        (Type::Object("fs_promises"), "exists") => Type::Function(
            vec![Type::String],
            Box::new(Type::Promise(Box::new(Type::Boolean))),
        ),
        // String namespace static — `String.fromCharCode(n)`.
        // `fromCodePoint` is the Unicode-aware sibling; in
        // tr's byte-Str layout the two collapse for code
        // points ≤ 0xff and ports keep arguments inside that
        // range to stay bun-portable.
        (Type::Object("String"), "fromCharCode" | "fromCodePoint") => {
            Type::Function(vec![Type::Number], Box::new(Type::String))
        }
        // §22.1.2.4 String.raw as a VALUE. The call forms -- direct
        // and tagged-template -- are claimed upstream by
        // `check_type_of_call_string_raw`, so this arm only ever
        // answers the value read (`String.raw.name`, `const r =
        // String.raw`). One declared parameter to match the spec
        // length; the substitutions are variadic and the cell's
        // dispatch reads them off argv, which is why the signature
        // never gates the call (the builtin-member-value rule).
        (Type::Object("String"), "raw") => Type::Function(vec![Type::Any], Box::new(Type::String)),
        _ => return None,
    };
    let _ = obj_ty;
    Some(Ok(ty))
}
