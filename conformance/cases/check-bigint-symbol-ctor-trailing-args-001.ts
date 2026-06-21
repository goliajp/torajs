// S308 — BigInt(value, ...trailing) + Symbol(desc?, ...trailing) callable-
// ctor trailing-arg ignore per ES §21.2.1 / §20.4.1. Spec coerces only
// args[0]; tora's strict gates rejected 2+ args with "BigInt(value)
// expects exactly 1 arg, got N" / "Symbol() expects 0 or 1 arg, got N".
// Widen check.rs + ssa_lower mirror with lower-and-drop loops.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

// BigInt(value, trailing)
calls = 0;
const b1 = BigInt(5, step("b1") as any);
console.log("BigInt calls=", calls, "v=", b1.toString());

calls = 0;
const b2 = BigInt("42", step("b2a") as any, step("b2b") as any);
console.log("BigInt-str calls=", calls, "v=", b2.toString());

calls = 0;
const b3 = BigInt(10n, step("b3a") as any, step("b3b") as any, step("b3c") as any);
console.log("BigInt-bigint calls=", calls, "v=", b3.toString());

// Symbol(desc, trailing)
calls = 0;
const s1 = Symbol("k", step("s1") as any);
console.log("Symbol-1 calls=", calls, "v=", s1.toString());

calls = 0;
const s2 = Symbol("k2", step("s2a") as any, step("s2b") as any);
console.log("Symbol-2 calls=", calls, "v=", s2.toString());

// Symbol() 0-arg + trailing — desc is optional in spec, so this is "Symbol
// with no useful args + trailing" — only typecheck reach but probe whether
// trailing fires. (skip — tora's typecheck rejects non-String desc; bare
// trailing without leading desc has nothing useful to land on. Skip.)
