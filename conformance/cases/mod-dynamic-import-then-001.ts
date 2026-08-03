// §13.3.10 ImportCall — statement-position `import("...")` is an
// expression statement (rotation 288: parse_stmt only takes the
// declaration parser when `import` is NOT followed by `(`), and the
// expression is a REAL `Promise.resolve(<ns>)` so `.then()` chains
// (the bare-namespace form it replaces answered a typecheck error).
import("./mod-dynamic-import-then-001-lib.ts").then((ns: any) => {
  console.log(ns.marker);
  console.log(ns.twice(21));
});
