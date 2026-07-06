// RFC 20260707 chunk 626 — a typed array passed into an `any[]`
// param (T-11 container widen at the call boundary). The checker
// admits `Array(T)` args into `Array(Any)` params; every lowering
// call-arg station pairs the admit with an elem-kind mark so the
// callee's kind-aware Arr<Any> readers decode the raw-slot layout.
// Pre-fix: every shape below was a loud checker reject ("argument
// 0: expected Array(Any), got Array(Number)").

// direct fn call, i64 elems
function f(xs: any[]): void {
  console.log(xs.length);
  console.log(xs[1]);
  for (const x of xs) console.log(x);
}
const nums: number[] = [10, 20, 30];
f(nums);

// f64 + bool + string elems through the same param
f([1.5, 2.5]);
const fs: number[] = [4.5, 5.5, 6.5];
f(fs);
const bs: boolean[] = [true, false];
f(bs);
const ss: string[] = ["a", "bb", "ccc"];
f(ss);

// class method param
class A {
  m(xs: any[]): number {
    console.log(xs[0]);
    return xs.length;
  }
}
console.log(new A().m(nums));

// closure (arrow) param
const g = (xs: any[]): void => {
  console.log(xs.length);
  console.log(xs[2]);
};
g(nums);

// fn-typed local indirect call
function h(xs: any[]): void {
  console.log(xs[0]);
}
const hv = h;
hv(fs);

// IIFE param
((xs: any[]): void => {
  console.log(xs[1]);
})(bs);

// nested typed array arg
const nest: number[][] = [
  [1, 2],
  [3, 4],
];
function n(xs: any[]): void {
  console.log(xs.length);
  const inner = xs[1];
  console.log(inner[0]);
}
n(nest);

// aliasing stays live: callee sees pushes made through the typed view
function len(xs: any[]): number {
  return xs.length;
}
nums.push(40);
console.log(len(nums));
console.log(nums[3]);
