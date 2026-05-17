// `var xs = [literal]` now keeps the Type::Arr<T> slot instead of
// hoisting to Type::Any. Prior behavior: var-hoist promoted every
// untyped `var` to a synth `let __: any = uninit` so pre-init
// reads could return undefined per ES §14.3.2 — but `arr.length`
// on a Type::Any slot dispatched through dynobj_get which has no
// `length` field, silently returning undefined. Array literals
// have a stable element type at parse time, so keeping the
// slot's static Array<T> type is the substrate-correct path; the
// pre-init undefined window for array vars is exotic enough that
// the typed-tier wins are the right tradeoff.
//
// Unlocks the dominant `var arr = [1, 2, 3]` test262 / plain-JS
// pattern: .length, indexed reads, .push / .indexOf / .join etc.
// all now work without an explicit annotation.

var ints = [10, 20, 30];
console.log(ints.length);    // 3
console.log(ints[0]);        // 10
console.log(ints[2]);        // 30
ints.push(40);
console.log(ints.length);    // 4
console.log(ints.indexOf(20));   // 1
console.log(ints.includes(30));  // true

var strs = ["alpha", "beta", "gamma"];
console.log(strs.length);            // 3
console.log(strs[1]);                // beta
console.log(strs.join(","));         // alpha,beta,gamma
console.log(strs.indexOf("gamma"));  // 2

// Mixed with explicit-typed let: both shapes coexist.
let arrLet: number[] = [1, 2];
arrLet.push(3);
console.log(arrLet.length);  // 3

// Inside a function body — var still hoists to the fn-root with
// its typed slot.
function check(): number {
  var inner = [100, 200];
  return inner.length;
}
console.log(check());        // 2
