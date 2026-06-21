// S332 — `s.{at,charAt,charCodeAt,codePointAt,repeat}(Any)` per ES
// §22.1.3.{1,2,3,4,17}. ToIntegerOrInfinity (or ToInteger for repeat
// count) accepts arbitrary-typed input. Pre-S332 the method-table
// sig `(Number) -> X` rejected `o: any` operands at typecheck with
// 'argument 0: expected Number, got Any'. The S222 carve-out only
// covered explicit-Undefined; Any fell through to the strict gate.
// Widen accepts Any; ssa_lower mirror routes Any through
// anyv_to_number → coerce_to_i64 → helper. Sister to S329
// (fromCharCode Any) / S331 (Array.indexOf fromIndex Any).

let counter = 0;
const step = (v: number): number => {
  counter += 1;
  return v;
};

const s = "Hello";

// any-from-binding (i64 carve-out)
const i: any = 1;
console.log("charAt=", s.charAt(i));
console.log("charCodeAt=", s.charCodeAt(i));
console.log("codePointAt=", s.codePointAt(i));
console.log("at=", s.at(i));

const n: any = 3;
console.log("repeat=", "ab".repeat(n));

// any-from-fn-return (side-effect counter — must fire 5×)
const giveIdx = (): any => step(2);
console.log("charAt(fn)=", s.charAt(giveIdx()));
console.log("charCodeAt(fn)=", s.charCodeAt(giveIdx()));
console.log("codePointAt(fn)=", s.codePointAt(giveIdx()));
console.log("at(fn)=", s.at(giveIdx()));
const giveN = (): any => step(2);
console.log("repeat(fn)=", "xy".repeat(giveN()));
console.log("counter=", counter);

// number-literal fast path still works
console.log("charAt-lit=", s.charAt(0));
console.log("charCodeAt-lit=", s.charCodeAt(0));
console.log("codePointAt-lit=", s.codePointAt(0));
console.log("at-lit=", s.at(-1));
console.log("repeat-lit=", "z".repeat(2));

// NaN-bearing Any → ToInteger(NaN) = 0
const nan: any = NaN;
console.log("charAt(NaN)=", s.charAt(nan));
console.log("charCodeAt(NaN)=", s.charCodeAt(nan));

// negative Any-idx
const neg: any = -1;
console.log("at(neg-any)=", s.at(neg));
