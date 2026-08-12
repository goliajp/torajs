// sloppy-goal `delete <bare name>` (§13.5.1.2, rotation 372):
// declared bindings and the non-configurable globals answer false;
// an unresolvable name answers true. CommonJS sloppy goal via the
// .cts extension (the bun mapping).
var x = 1;
console.log(delete x, x);
// @ts-ignore
console.log(delete zzz_undeclared);
// (`delete Infinity` / `delete undefined`: spec §18.1 says false —
// non-configurable globals — and test262 S15.1.1.*_A3_T2 assert
// exactly that; bun's CJS-sloppy answers true there, a quirk this
// fixture leaves out since its oracle IS bun. NaN agrees.)
console.log(delete NaN);
function f() { return 7; }
console.log(delete f, typeof f);
var z = 3;
console.log(delete delete z);
let w = 4;
console.log(delete w, w);
