// P13-S5 — dynamic `import("./lib")` expression per ES §13.3.10. Pre-fix
// tora's parser rejected `import` in expression context with
//   `parse error: expected expression, got Import`
//
// Substrate fix (P13-S5):
// - parser.rs: parse_primary detects `import(<string-literal>)` and
//   synthesizes an `import * as __dyn_ns_<n> from "<source>"` decl
//   (via synth_classes, flushed before the enclosing stmt), then
//   returns the expression as `{ value: __dyn_ns_<n> }` — a plain
//   object literal whose `value` field is the namespace struct.
//   Tora's `await <expr>` desugar (`<expr>.value`) reads the
//   namespace through that wrapper without the typecheck having to
//   traverse Promise<struct-with-fn-fields> (which is a separate
//   substrate gap tracked in L3b — only matters for `.then()` use,
//   which is uncommon for dynamic import in TS code).
//
// Subset constraint: source must be a string literal so the
// resolver can statically materialize the namespace at compile
// time. Non-literal sources (e.g., `import(pathVar)`) are L3b — the
// AOT runtime has no file-loading machinery, so the only viable
// extension is a build-time URL whitelist.

const mod = await import("./mod-dynamic-import-001-lib.ts");
console.log(mod.compute(4));    // 17
console.log(mod.PREFIX);        // "dyn:"
console.log(mod.PREFIX + mod.compute(10));    // "dyn:101"
