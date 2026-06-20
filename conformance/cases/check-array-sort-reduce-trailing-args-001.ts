// S276 — Array.{sort,toSorted}(cmp, ...trailing) per ES
// §23.1.3.{30,33}; Array.{reduce,reduceRight}(cb, init, ...trailing)
// per ES §23.1.3.{22,23}. Spec uses cmp / (cb + init) only; trailing
// silent-drop.
//
// Pre-S276 tora rejected:
//   xs.toSorted(cmp, "trailing")  → "expected 1 argument(s), got 2"
//   xs.reduceRight(cb, 0, "x", "y") → "expected 2 argument(s), got 4"
//
// ssa_lower mirror evals-and-drops trailing exprs so side effects
// (step counter) fire.

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

const xs = [5, 1, 3, 2, 4];

// sort/toSorted trailing
console.log(xs.toSorted((a: number, b: number) => a - b, step("a")));
console.log(xs.toSorted((a: number, b: number) => a - b, step("b"), step("c")));
console.log([...xs].sort((a: number, b: number) => b - a, step("d"), step("e")));

// reduce/reduceRight trailing
const ys = [1, 2, 3];
console.log(ys.reduce((acc: number, x: number) => acc + x, 0, step("f")));
console.log(ys.reduce((acc: number, x: number) => acc + x, 0, step("g"), step("h")));
console.log(ys.reduceRight((acc: number, x: number) => acc + x, 10, step("i")));
console.log(ys.reduceRight((acc: number, x: number) => acc + x, 10, step("j"), step("k"), step("l")));

console.log("calls=" + calls);
