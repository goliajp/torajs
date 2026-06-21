// S333 — `s.{slice,substring,substr}(Any [, Any])` per ES
// §22.1.3.{20,22,23}: ToIntegerOrInfinity accepts arbitrary-typed
// input on both positional slots. Pre-S333 the 1-arg / 2-arg
// carve-outs in check.rs (lines ~7290 / ~7320) rejected
// `o: any` operands with 'String.slice arg must be number, got
// Any'; for substr the method-table sig `(Number, Number) -> String`
// rejected at the strict gate. Widen accepts Any; ssa_lower
// mirror routes Any through anyv_to_number → coerce_to_i64 →
// helper. Sister to S332 (charAt/charCodeAt/... Any).

let counter = 0;
const step = (v: number): number => {
  counter += 1;
  return v;
};

const s = "Hello";

// any-from-binding — 2-arg form
const a: any = 1;
const b: any = 3;
console.log("slice(a,b)=", s.slice(a, b));
console.log("substring(a,b)=", s.substring(a, b));
console.log("substr(a,b)=", s.substr(a, b));

// 1-arg form
console.log("slice(a)=", s.slice(a));
console.log("substring(a)=", s.substring(a));
console.log("substr(a)=", s.substr(a));

// any-from-fn-return — side-effect counter must fire 6×
const giveStart = (): any => step(1);
const giveEnd = (): any => step(4);
console.log("slice(fn,fn)=", s.slice(giveStart(), giveEnd()));
console.log("substring(fn,fn)=", s.substring(giveStart(), giveEnd()));
console.log("substr(fn,fn)=", s.substr(giveStart(), giveEnd()));
console.log("counter=", counter);

// number-literal fast path still works
console.log("slice-lit=", s.slice(0, 2));
console.log("substring-lit=", s.substring(0, 2));
console.log("substr-lit=", s.substr(0, 2));

// negative Any-idx (slice supports neg; substring clamps; substr neg start = max(0,len+start))
const neg: any = -2;
console.log("slice(neg)=", s.slice(neg));
console.log("substring(neg)=", s.substring(neg));
console.log("substr(neg)=", s.substr(neg));

// NaN-bearing Any → ToInteger(NaN) = 0
const nan: any = NaN;
console.log("slice(nan,nan)=", s.slice(nan, nan));
console.log("substring(nan,nan)=", s.substring(nan, nan));

// mixed Number + Any
const e: any = 4;
console.log("slice(0,any)=", s.slice(0, e));
console.log("substring(0,any)=", s.substring(0, e));
console.log("substr(0,any)=", s.substr(0, e));
