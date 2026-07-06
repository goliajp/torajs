// L3b #2 / RFC 20260706-typed-arr-any-escape B3 — cross-module escape:
// an imported typed array aliased into any in the importing module.
// The escape seed (any let-init) lives here; the alloc site lives in
// the lib module. Module-level analysis unions the import binding into
// the same alias class, so the lib's alloc demotes to Arr<Any> and the
// kind-changing write is accepted with full read-surface parity.
//
// Grow through the any alias (u.push) is NOT exercised: that is the
// known any-alias grow write-back gap (recv-slot-only realloc
// write-back; independent RFC lane).
import { t } from "./escape-any-002-xmod-lib.ts";
const u: any = t;
u[0] = "s";
console.log(t[0]);
console.log(u[0]);
console.log(t.length);
for (const x of t) console.log(x);
console.log(t.join(","));
console.log(t[0] + 1);
