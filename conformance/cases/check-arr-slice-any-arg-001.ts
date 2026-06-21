// S334 — `xs.slice(Any [, Any])` per ES §23.1.3.28:
// ToIntegerOrInfinity accepts arbitrary-typed input on both
// positional slots. Pre-S334 the check.rs carve-out at line ~6398
// rejected `o: any` operands with 'Array.slice arg must be number,
// got Any'. Widen accepts Any; ssa_lower mirror routes Any
// through anyv_to_number → coerce_to_i64 → helper. Sister to
// S332 (str charAt/charCodeAt/... Any), S333 (str slice Any).

let counter = 0;
const step = (v: number): number => {
  counter += 1;
  return v;
};

const xs = [1, 2, 3, 4, 5];

// any-from-binding (2-arg)
const a: any = 1;
const b: any = 3;
console.log("slice(a,b)=", xs.slice(a, b));

// 1-arg form
console.log("slice(a)=", xs.slice(a));

// any-from-fn-return (side-effect counter must fire 2×)
const giveStart = (): any => step(1);
const giveEnd = (): any => step(4);
console.log("slice(fn,fn)=", xs.slice(giveStart(), giveEnd()));
console.log("counter=", counter);

// number-literal fast path still works
console.log("slice-lit=", xs.slice(0, 2));

// negative Any-idx (slice supports neg)
const neg: any = -2;
console.log("slice(neg)=", xs.slice(neg));

// NaN-bearing Any → ToInteger(NaN) = 0
const nan: any = NaN;
console.log("slice(nan,nan)=", xs.slice(nan, nan));

// mixed Number + Any
const e: any = 4;
console.log("slice(0,any)=", xs.slice(0, e));

// String array
const ss = ["a", "b", "c", "d"];
const i: any = 1;
const j: any = 3;
console.log("str-slice=", ss.slice(i, j));
