// P-PARSE.2 — nested destructuring patterns in fn / arrow params
// per ES spec §14.1.3. Pre-fix tora's parse_destr_param accepted
// only flat patterns (`[a, b]`, `{ x, y }`); a nested `[` or
// `{` in element / value position triggered 'expected
// identifier in array param destructuring, got LBracket'. Test262's
// arrow-function/dstr / function/dstr suites use these shapes
// pervasively (~50+ cases hit this).
//
// Implementation: parse_destr_param now defers to a recursive
// parse_destr_into helper that splits array-vs-object dispatch
// into two siblings, parse_destr_array_into and
// parse_destr_object_into. Each leaf binding emits a
// `let leaf = <src>[i]` (array) or `let leaf = <src>.<field>`
// (object) into `lets`; each nested sub-pattern synthesizes a
// fresh `__nested_destr_<id>` binding, emits the load into it,
// and recurses with the new source name. The flat MVP is the
// depth-1 case of this recursion — no behaviour change for
// existing fixtures.
//
// Scope: parser-level acceptance + lower-time execution when
// the typed shape matches. Untyped-tier inference for nested
// destr is a P0 / P3 follow-up — without explicit type
// annotations the inner load can't pick the right element type.
// Fixtures here use explicit typed shapes so both parse and
// run succeed.

// Object inside object — typed.
type AB = { a: { b: number } }
function objInObj({ a: { b } }: AB): void {
  console.log(b)
}
objInObj({ a: { b: 42 } })                 // 42

// Array inside object — typed.
type WithArr = { x: number[] }
function arrInObj({ x: [a, b] }: WithArr): void {
  console.log(a)
  console.log(b)
}
arrInObj({ x: [10, 20] })                  // 10 / 20

// Object inside array — typed.
type Item = { x: number }
function objInArr([{ x }]: Item[]): void {
  console.log(x)
}
objInArr([{ x: 7 }])                       // 7

// Multi-field nested object — typed.
type Doubly = { a: { x: number, y: number }, b: { z: number } }
function multi({ a: { x, y }, b: { z } }: Doubly): void {
  console.log(x)
  console.log(y)
  console.log(z)
}
multi({ a: { x: 11, y: 22 }, b: { z: 33 } })  // 11 / 22 / 33

// Regression — flat array destr still parses + runs unchanged.
function flat([a, b, c]: number[]): void {
  console.log(a)
  console.log(b)
  console.log(c)
}
flat([4, 5, 6])                            // 4 / 5 / 6

// Regression — flat object destr w/ rename still works.
type XY = { x: number, y: number }
function flatObj({ x: foo, y: bar }: XY): void {
  console.log(foo)
  console.log(bar)
}
flatObj({ x: 100, y: 200 })                // 100 / 200
