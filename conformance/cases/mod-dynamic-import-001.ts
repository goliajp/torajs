// P13-S5 — dynamic `import("./lib")` expression per ES §13.3.10. Pre-fix
// tora's parser rejected `import` in expression context with
//   `parse error: expected expression, got Import`
//
// Substrate fix (P13-S5, reshaped rotation 233):
// - parser.rs: parse_primary detects `import(<string-literal>)` and
//   synthesizes an `import * as __dyn_ns_<n> from "<source>"` decl
//   (via synth_classes, flushed before the enclosing stmt), then
//   returns the namespace struct itself. `await import(...)` passes
//   it through identity (await dispatches by type; the namespace is
//   not a Promise), without the typecheck traversing
//   Promise<struct-with-fn-fields>. Standalone `.then()` use stays
//   a separate L3b gap — uncommon for dynamic import in TS code.
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
