// S329 — `String.fromCharCode(Any)` per ES §22.1.2.1 ToUint16. Each
// arg goes through ToUint16, which coerces arbitrary-typed input
// (Number, Boolean, NaN→0, etc.). Pre-S329 the method-table sig
// `(Number) -> String` rejected `o: any` operands at typecheck with
// 'argument 0: expected Number, got Any', blocking the
// `const o: any = code; String.fromCharCode(o)` shape outright.
// Widen accepts Any; ssa_lower mirror routes Any through
// anyv_to_number → coerce_to_i64 → helper. fromCodePoint stays
// Number-only because its RangeError throw shape needs separate
// substrate (L3b).

const codeA: any = 65;
const a = String.fromCharCode(codeA);
console.log("a=", a);

const codeBC: any = 66;
const b = String.fromCharCode(codeBC);
console.log("b=", b);

// Number-typed still works (i64 fast path)
const c = String.fromCharCode(67);
console.log("c=", c);

// Any from a fn return
const giveCode = (): any => 68;
const d = String.fromCharCode(giveCode());
console.log("d=", d);

// Explicit `undefined` literal still folds to ConstI64(0) → " " (S231)
const e = String.fromCharCode(undefined);
console.log("e.charCodeAt=", e.charCodeAt(0));
