// P13-S3 — bare side-effect import (`import "./lib"`) per ES §16.2.2.
//
// Pre-fix tora rejected the form at K.2:
//   `import error: side-effect-only import (\`import "./lib"\`) not
//   supported in K.2`
//
// Substrate fix (P13-S3):
// - check_k2_form drops the named-empty + default-none reject
// - WorkItem carries a `side_effect_only` flag
// - Lib walk under the flag injects every non-ImportDecl top-level
//   stmt (including bare expressions, classes, lets) in source order
//   instead of dropping non-export ones

import "./mod-side-effect-import-001-lib.ts";
console.log("main");
