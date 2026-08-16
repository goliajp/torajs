// §16.2.3 + §16.2.1.6 — `import * as ns` of a module whose exports
// arrive through `export * from`. The star's target is a SEPARATE BFS
// work item popped after the hub, so the namespace object cannot be
// built at the end of the hub's walk: r421 moved namespace fields into
// a per-ALIAS accumulator and materializes every `let ns = { … }` once
// the whole BFS has drained.
//
// Without that, `ns` would hold only the hub's own `NB` and every
// star-reached name would be missing — silently, since a struct-typed
// namespace just wouldn't have the field.
import * as ns from "./b.ts";
console.log(ns.NB);
console.log(ns.NA);
console.log(ns.fna());
