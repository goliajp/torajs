// P-PARSE.6 — three parser items that all show up in the same
// arrow-function/dstr / function/dstr suites in test262 and
// together account for ~90 of the 120 remaining parse errors
// in the language/expressions 500-sample after P-PARSE.1-5:
//
//   (a) rest pattern in array destructuring slot:
//       `function r([first, ...rest]) {...}`
//       per ES spec §13.15.5.3 step 4i — RestPattern collects
//       the remaining iterator values into a fresh Array. tora
//       desugars it to `let rest = src.slice(elem_idx)`.
//
//   (b) elision (extra commas) in array destructuring pattern:
//       `function elision([a, , c]) {...}` — skip middle slot,
//       no binding emitted, just bump elem_idx.
//
//   (c) whole-pattern default on a destr param:
//       `function g({x, y} = {x:1, y:2}) {...}` — applied
//       through the existing Param.default plumbing so the
//       destr lets see the fallback value when the slot is
//       undefined. Wires the fix into all three call sites
//       of parse_destr_param: parse_fn, parse_arrow_fn,
//       class-method parse_param_list.
//
// Also lifts the corresponding error message (used to be
// 'expected `,` or `)` after destr param, got Eq') —
// 90+ test262 cases that hit this exact wording start passing.

// (a) Rest pattern.
function r([first, ...rest]: number[]): void {
  console.log(first)
  console.log(rest.length)
}
r([1, 2, 3])                                 // 1 / 2
r([10])                                      // 10 / 0

// (b) Elision in destr pattern.
function elision([a, , c]: number[]): void {
  console.log(a)
  console.log(c)
}
elision([1, 2, 3])                           // 1 / 3
elision([10, 20, 30, 40])                    // 10 / 30

// (c) Whole-pattern default on arrow fn.
let f = ([a, b]: number[] = [10, 20]): number => a + b
console.log(f([3, 4]))                       // 7
console.log(f())                             // 30

// (c) Whole-pattern default on declared fn.
function g({x, y}: {x: number, y: number} = {x: 99, y: 100}): number {
  return x + y
}
console.log(g({x: 1, y: 2}))                 // 3
console.log(g())                             // 199

// Combination — nested destr + whole-pattern default + leaf
// default at inner position. All three plumbing paths
// composing through the same recursion.
function combo({a, b = 7}: {a: number, b: number} = {a: 100, b: 200}): void {
  console.log(a)
  console.log(b)
}
combo({a: 1, b: 2})                          // 1 / 2
combo()                                      // 100 / 200

// Regression — flat destr w/o defaults / rest still works.
function flat([a, b]: number[]): void {
  console.log(a)
  console.log(b)
}
flat([5, 6])                                 // 5 / 6
