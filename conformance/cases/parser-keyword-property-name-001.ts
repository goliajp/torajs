// V3-18 wedge — reserved-word tokens are accepted as property
// names per ES spec §12.7.6 (PropertyName allows IdentifierName,
// which includes the full reserved-word list). Affects four
// positions in the parser:
//   1. object-literal field names: `{ type: ..., default: ... }`
//   2. member access:               `obj.type`, `obj.return`
//   3. object-destructuring field:  `let { type: t } = o`
//   4. class member names:          `class C { type = ...; default() {} }`
//
// Pre-fix all four positions had their own short keyword
// whitelist that drifted apart over time. Centralized into a
// single `keyword_property_name` helper covering the full
// reserved-word list (catch / finally / return / throw / if /
// else / for / while / do / break / continue / switch / case /
// default / class / new / this / function / typeof / instanceof /
// try / yield / type / async / await / import / export / null /
// true / false / let / const / extends / super / void).

// Object-literal field names with reserved words.
let o = {
  type: "alpha",
  default: 42,
  return: "ok",
  class: "C",
  new: 1,
  if: 2,
  for: 3,
  switch: "s",
  catch: "c",
  finally: "f",
}
console.log(o.type)                    // alpha
console.log(o.default)                 // 42
console.log(o.return)                  // ok
console.log(o.class)                   // C
console.log(o.new)                     // 1
console.log(o.if)                      // 2
console.log(o.for)                     // 3
console.log(o.switch)                  // s
console.log(o.catch)                   // c
console.log(o.finally)                 // f

// Object destructuring with reserved-word fields (rename
// required since the bound binding name itself can't be a
// reserved word).
let { type: t, default: d, return: r } = o
console.log(t, d, r)                   // alpha 42 ok

// Class member name as a reserved word — both field and method.
class Trie {
  type = "trie"
  count = 0
  add(): void { this.count++ }
  default(): string { return "x" }
}
let tr = new Trie()
tr.add(); tr.add(); tr.add()
console.log(tr.type, tr.count)         // trie 3
console.log(tr.default())              // x

// Extended reserved-word set (async / await / void / super).
let o2 = { async: true, await: false, void: 9, super: "s" }
console.log(o2.async, o2.await, o2.void, o2.super)
                                       // true false 9 s

// Keywords reachable through member access on a class instance.
class T {
  async = true
  void = 99
  type = "k"
}
let tt = new T()
console.log(tt.async, tt.void, tt.type)
                                       // true 99 k
