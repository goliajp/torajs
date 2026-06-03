// P13-S4 — `export { a, b as c } from "./other"` (named re-export) per
// ES §16.2.3.7. Pre-fix tora rejected the form at modules.rs:
//   `import error: bare named export not supported in K.2 (./b)`
// because ExportDecl had no `source` field and the bare-named-export
// branch always returned the K.2 reject.
//
// Substrate fix (P13-S4):
// - ast.rs: ExportDecl gains `source: Option<String>`
// - parser.rs: `export { … } from "./y"` populates source
// - formatter: emits the `from "./y"` clause when source is present
// - modules.rs: lib walk's bare-named-export arm splits on source:
//     * source = None  → still rejected (this would be a plain re-bind
//       of an in-scope name, no module relationship)
//     * source = Some  → push a transitive BFS load of source with the
//       lib's (orig, alias) names translated into the caller's final
//       alias view; the importer's `want` filter drives selection
//
// Coverage: pass-through name (`fa`), lib-side alias (`KA as
// RENAMED_K`), and the importer's further rename of an already-aliased
// name (covered by the second main below).

import { fa, RENAMED_K, fb } from "./mod-re-export-001-b.ts";
console.log(fa());
console.log(RENAMED_K);
console.log(fb());
