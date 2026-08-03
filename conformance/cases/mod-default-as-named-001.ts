// §16.2.2 ImportSpecifier — ModuleExportName covers reserved words,
// so `import { default as x }` re-binds the default export under a
// local name (equivalent to `import x from ...`). Pre-fix the named
// clause only accepted Token::Ident:
//   `parse error: expected ident in import named clause, got Default`
//
// Substrate fix (rotation 289): the named-clause loop accepts
// Token::Default as the export name; `default as x` then rides the
// resolver's existing default lane (it IS the default binding).
// Bare `{ default }` stays a syntax error — `default` is a reserved
// word, not a legal ImportedBinding.

import { default as shout, NAMED_Y } from "./mod-default-as-named-001-lib.ts";

console.log(shout());     // "default-as-named"
console.log(NAMED_Y);     // 7
