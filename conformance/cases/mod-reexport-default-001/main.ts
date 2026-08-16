// §16.2.3 — `default` on either side of an export specifier, plus the
// ModuleExportName spellings the parser used to reject (`as default`
// died on `expected alias ident`, `{ default } from` on `expected
// ident`). Each side of the specifier picks a different resolver
// lane; see the hubs.
import d, { H } from "./hub.ts";
import d2, { X } from "./hub2.ts";
console.log(d);
console.log(H);
console.log(X);
console.log(d2);
