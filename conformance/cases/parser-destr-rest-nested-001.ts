// P-PARSE.7 — two leftover destructuring shapes from the
// language/expressions 500-sample after P-PARSE.6 still hits
// 30+ parse-errors:
//
//   (a) Rest target is itself a destructuring pattern:
//       `[a, ...[b, c]]`     — rest = src.slice(idx); destr [b,c]
//       `[a, ...{length}]`   — rest = src.slice(idx); destr {length}
//       Per ES spec §13.15.5.3 step 4i.iii — RestPattern accepts
//       a BindingPattern, not just a BindingIdentifier. tora
//       was rejecting with 'expected identifier after `...` in
//       array param destructuring, got LBracket / LBrace'.
//
//   (b) Object-destr field whose value is a nested pattern WITH
//       a default:
//       `{ x: [a, b] = [1, 2] }`
//       Mirror of the array-destr nested-default order fix from
//       P-PARSE.6 — parse the nested body first so the trailing
//       `=` becomes visible to maybe_parse_object_destr_default.
//
// Both share the same restructure: parse the inner pattern body
// into a temp buffer, peek for `= DEFAULT`, then emit the
// synth binding (with the defaulted load expression) and the
// body lets in source order.

// (a) Rest with object-destr target — the canonical "take the
// length of the tail" pattern.
function r1([a, ...{length}]: number[]): void {
  console.log(a)
  console.log(length)
}
r1([1, 2, 3, 4])                             // 1 / 3

// (a) Rest with array-destr target — the canonical "split first
// vs the rest split into b, c" pattern.
function r2([a, ...[b, c]]: number[]): void {
  console.log(a)
  console.log(b)
  console.log(c)
}
r2([1, 2, 3])                                // 1 / 2 / 3

// (b) Object-destr nested + default.
function obj({x: [a, b] = [10, 20]}: {x: number[]}): void {
  console.log(a)
  console.log(b)
}
obj({x: [3, 4]})                             // 3 / 4

// Combination — nested rest inside an outer destr.
function combo([first, ...[second, ...third]]: number[]): void {
  console.log(first)
  console.log(second)
  console.log(third.length)
}
combo([1, 2, 3, 4, 5])                       // 1 / 2 / 3

// Regression — flat rest still works after the new path lands.
function flat([h, ...t]: number[]): void {
  console.log(h)
  console.log(t.length)
}
flat([10, 20, 30])                           // 10 / 2
