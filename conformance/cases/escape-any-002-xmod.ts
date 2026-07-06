// L3b #2 / RFC 20260706-typed-arr-any-escape B3 — cross-module escape:
// an imported typed array aliased into any in the importing module.
// The escape seed (any let-init) lives here; the alloc site lives in
// the lib module. Module-level analysis unions the import binding into
// the same alias class, so the lib's alloc demotes to Arr<Any> and the
// kind-changing write is accepted with full read-surface parity.
//
// Grow through the any alias (u.push) reads back through the original
// binding since RFC 20260706-arr-grow-alias-stability B1 (the cell is
// fixed across grow; all aliases observe growth).
import { t } from "./escape-any-002-xmod-lib.ts";
const u: any = t;
u[0] = "s";
console.log(t[0]);
console.log(u[0]);
console.log(t.length);
for (const x of t) console.log(x);
console.log(t.join(","));
console.log(t[0] + 1);
u.push(true);
console.log(t.length);
console.log(t.join(","));
for (const x of t) console.log(x);
