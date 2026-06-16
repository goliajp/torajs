// S131-2 narrow: top-level `: Date` type-ann acceptance. Pre-fix,
// `const d: Date = new Date(0)` was rejected by ssa_lower_parse_type
// with "unsupported type annotation `Date`" — check_type_ann's
// builtin name registry accepted the lower-case `date` (the internal
// spelling type_to_ann emits) but not the TS-spelled `Date`. Mirror
// of the existing `regex | RegExp → Type::RegExp` and `weakmap |
// WeakMap → Type::WeakMap` arms.
//
// Class-field `: Date` init inference (`class W { d: Date = new
// Date(0) }`) is a separate substrate gap — desugar_classes rewrites
// `new Date(x)` to `__torajs_date_from_ms(x)` per ast.rs:2144, but
// the class-field init typecheck still reports `Number` for the
// rewritten Call result, leaving __this struct field-type mismatch.
// Deferred to L3b.
const d: Date = new Date(0);
console.log(d.getTime());
console.log(d.getUTCFullYear());
console.log(d.toISOString());

const d2: Date = new Date(86400000);
console.log(d2.getTime());

const d3: Date = new Date("1970-01-01T00:00:00.000Z");
console.log(d3.getTime());
