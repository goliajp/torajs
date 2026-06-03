// P13-S1 — `export default <expr>` paired with `import x from "./lib"`
// (default-binding import) per ES §16.2.3 / §16.2.2.
//
// Pre-fix tora rejected the default form at the K.2 modules resolver:
//   `import error: default import (\`import x from "./lib"\`) not
//   supported in K.2`
//
// Substrate fix (P13-S1):
// - modules.rs `check_k2_form` allows default
// - modules.rs WorkItem carries the importer's default alias
// - lib walk converts `ExportDecl { default_expr: Some(eid) }` into a
//   synthetic `let <importer-alias> = <expr>` for injection

import greet, { NAMED_X } from "./mod-default-import-001-lib.ts";

console.log(greet());     // "default-result"
console.log(NAMED_X);     // 42
