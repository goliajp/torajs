// S338 — `s.{padStart,padEnd}(Any [, fillStr])` per ES §22.1.3.
// {16,17} step 1: ToLength accepts arbitrary-typed input on the
// maxLength slot. Pre-S338 the check.rs carve-out at ~7244
// rejected `o: any` operands with 'String.padStart arg 0 must
// be number, got Any'. Widen accepts Any; ssa_lower mirror
// routes Any through anyv_to_number → coerce_to_i64 → helper.
// Sister to S332/S333. fillStr (args[1]) stays String-only —
// dynamic-typed fillStr substrate is a separate L3b item.

let counter = 0;
const step = (v: number): number => {
  counter += 1;
  return v;
};

// any-from-binding (1-arg + 2-arg)
const n: any = 5;
console.log("padStart(n)=", "ab".padStart(n));
console.log("padEnd(n)=", "ab".padEnd(n));
console.log("padStart(n,'0')=", "ab".padStart(n, "0"));
console.log("padEnd(n,'-')=", "ab".padEnd(n, "-"));

// any-from-fn-return — side-effect counter must fire
const giveN = (): any => step(6);
console.log("padStart-fn=", "x".padStart(giveN(), "*"));
console.log("padEnd-fn=", "x".padEnd(giveN(), "*"));
console.log("counter=", counter);

// number-literal fast path still works
console.log("padStart-lit=", "z".padStart(4, "0"));
console.log("padEnd-lit=", "z".padEnd(4, "0"));

// NaN-bearing Any → ToLength(NaN) = 0 → no-op
const nan: any = NaN;
console.log("padStart-nan=", "ab".padStart(nan, "X"));
console.log("padEnd-nan=", "ab".padEnd(nan, "X"));

// negative Any → ToLength(neg) = 0 → no-op
const neg: any = -3;
console.log("padStart-neg=", "ab".padStart(neg, "X"));
console.log("padEnd-neg=", "ab".padEnd(neg, "X"));
