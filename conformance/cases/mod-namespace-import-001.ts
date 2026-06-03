// P13-S2 — `import * as M from "./lib"` namespace import per ES §16.2.2.
//
// Pre-fix tora rejected the form at modules.rs:
//   `import error: namespace import (\`import * as ns from "./lib"\`)
//   not supported in K.2`
//
// Substrate fix (P13-S2):
// - check_k2_form drops the namespace reject (modules.rs now allows
//   all four import shapes; bare `export { a } from "./b"` re-export
//   stays gated as the P13-S4 surface)
// - WorkItem carries `namespace_alias: Option<String>` through BFS
// - Lib walk injects every value export under its original name (no
//   `want` filter, no rename) and accumulates them into a names list
// - After the walk a synthetic `let <alias> = { name1: name1, ... }`
//   object literal lands as a new LetDecl; struct-field-method
//   dispatch then routes `M.fn()` / `M.X` to the lib's symbols.

import * as math from "./mod-namespace-import-001-lib.ts";
console.log(math.add(5, 3));
console.log(math.sub(10, 4));
console.log(math.VERSION);
console.log(math.SCALE);
console.log(math.add(math.SCALE, math.sub(7, 2)));
