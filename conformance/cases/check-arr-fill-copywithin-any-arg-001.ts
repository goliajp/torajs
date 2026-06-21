// S335 — `xs.fill(v, Any [, Any])` per ES §23.1.3.7 step 5/9 and
// `xs.copyWithin(Any [, Any [, Any]])` per ES §23.1.3.4:
// ToIntegerOrInfinity accepts arbitrary-typed input on every
// positional index slot. Pre-S335 the check.rs carve-outs at
// ~6444 / ~7174 rejected `o: any` operands with 'Array.fill arg
// 1 (start) must be number, got Any' / 'Array.copyWithin arg 0
// must be number, got Any'. Widen accepts Any; ssa_lower mirror
// routes Any through anyv_to_number → coerce_to_i64 →
// relative_to_len → helper. Sister to S334 (Array.slice Any).

let counter = 0;
const step = (v: number): number => {
  counter += 1;
  return v;
};

// Array.fill ---------
const a1: any = 1;
const a2: any = 3;
console.log("fill(0,a,b)=", [10, 10, 10, 10, 10].fill(0, a1, a2));
console.log("fill(0,a)=", [10, 10, 10].fill(0, a1));
console.log("fill(0)=", [10, 10, 10].fill(0));

// Array.copyWithin ---------
const t: any = 0;
const s: any = 2;
const e: any = 4;
console.log("cw(t,s,e)=", [1, 2, 3, 4, 5].copyWithin(t, s, e));
console.log("cw(t,s)=", [1, 2, 3, 4, 5].copyWithin(t, s));
console.log("cw(t)=", [1, 2, 3, 4, 5].copyWithin(t));

// any-from-fn-return (side-effect counter must fire)
const giveIdx = (): any => step(1);
console.log("fill(fn)=", [9, 9, 9, 9].fill(0, giveIdx()));
console.log("cw(fn,fn,fn)=", [1, 2, 3, 4, 5].copyWithin(giveIdx(), giveIdx(), giveIdx()));
console.log("counter=", counter);

// negative Any-idx
const neg: any = -2;
console.log("fill-neg=", [1, 2, 3, 4, 5].fill(0, neg));
console.log("cw-neg-target=", [1, 2, 3, 4, 5].copyWithin(neg, 0));

// NaN-bearing Any → ToInteger(NaN) = 0
const nan: any = NaN;
console.log("fill(nan,nan)=", [1, 2, 3, 4, 5].fill(0, nan, nan));
console.log("cw(nan,nan)=", [1, 2, 3, 4, 5].copyWithin(nan, nan));

// literal-Number fast path still works
console.log("fill-lit=", [1, 2, 3, 4, 5].fill(0, 1, 3));
console.log("cw-lit=", [1, 2, 3, 4, 5].copyWithin(0, 2, 4));
