// §13.3.10 ImportCall inside an async fn body. Pre-fix the parser
// synthesized `import * as __dyn_ns_0` whose namespace materialized
// as a top-level MAIN-LOCAL `let` — invisible from the lifted async
// fn body, so `await import(...)` answered
//   `type error: unknown identifier __dyn_ns_0`
//
// Substrate fix (rotation 289): dynamic-import namespaces skip the
// `let` entirely — the resolver rewrites each `Ident(__dyn_ns_<n>)`
// use site into the namespace object literal in place
// (`inline_dyn_ns_objlits`), whose field Idents resolve against the
// injected lib decls from any fn body (FnDecls via the pass-1 hoist,
// literal consts via the pass-2 pre-pass). Second `import()` of an
// already-visited module reuses the remembered field list (the
// request itself is sentinel-deduped to nothing).

async function first(): Promise<void> {
  const m: any = await import("./mod-dynamic-import-async-001-lib.ts");
  console.log(m.marker);
  console.log(m.twice(21));
}
async function second(): Promise<void> {
  const m: any = await import("./mod-dynamic-import-async-001-lib.ts");
  console.log(m.twice(5));
}
function third(): void {
  import("./mod-dynamic-import-async-001-lib.ts").then((ns: any) => {
    console.log(ns.twice(7));
  });
}
await first();
await second();
third();
