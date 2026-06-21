// S305 — Number.is{Integer,NaN,Finite,SafeInteger} + global isNaN/isFinite
// (value, ...trailing) trailing-arg ignore per ES §21.1.2.{2,3,4,5} /
// §19.2.{3,4}. check.rs S253/S252 already typecheck-and-drop; ssa_lower's
// arms (Number.is*:17774 / global isNaN/isFinite:17241) lowered only
// args[0] and silent-dropped trailing — step()-counter probe falsified
// prior "covered" assumption (bun calls 1-2x, tora calls 0x).

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

// Number.isInteger
calls = 0;
const a1 = Number.isInteger(5, step("ni-i1") as any);
console.log("N.isInt-1 calls=", calls, "v=", a1);

calls = 0;
const a2 = Number.isInteger(5.5, step("ni-i2a") as any, step("ni-i2b") as any);
console.log("N.isInt-2 calls=", calls, "v=", a2);

// Number.isNaN
calls = 0;
const a3 = Number.isNaN(NaN, step("ni-n1") as any);
console.log("N.isNaN calls=", calls, "v=", a3);

// Number.isFinite
calls = 0;
const a4 = Number.isFinite(5, step("ni-f1") as any, step("ni-f2") as any);
console.log("N.isFin calls=", calls, "v=", a4);

// Number.isSafeInteger
calls = 0;
const a5 = Number.isSafeInteger(5, step("ni-s1") as any);
console.log("N.isSafe calls=", calls, "v=", a5);

// global isNaN
calls = 0;
const a6 = isNaN(NaN, step("g-n1") as any);
console.log("isNaN calls=", calls, "v=", a6);

// global isFinite
calls = 0;
const a7 = isFinite(5, step("g-f1") as any, step("g-f2") as any);
console.log("isFin calls=", calls, "v=", a7);
