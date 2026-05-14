// P-PARSE.3 — destructuring patterns with default values per
// ES spec §13.15.5.3 / §13.15.5.4. Pre-fix tora's parser bailed
// at the `=` inside any destr position with 'expected `,` or `]`
// in array param destructuring, got Eq' / similar for objects.
// Test262's arrow-function/dstr / function/dstr suites use
// defaults pervasively (~30+ cases hit this single shape).
//
// Implementation: parse_destr_array_into and
// parse_destr_object_into both peek for `=` after binding the
// slot name and consume a default expression when present. The
// generated AST wraps the load:
// * array slot at index i:
//     src.length > i ? src[i] : DEFAULT
//   (per spec the default fires when iterator is "done" at
//   index i; for tora's fixed-length arrays this collapses to
//   the length check.)
// * object field:
//     mem === null ? DEFAULT : mem
//   (per spec the default fires when the value is undefined;
//   tora has no real undefined yet — P1 — so we substitute
//   null. For non-Nullable struct fields the default is dead
//   code and observable behaviour matches bun.)
//
// Subset notes:
// * Spec also fires the default for `undefined` value; once
//   P1 ships real Type::Undefined the cond should also test
//   `=== undefined`.
// * Object destr with a Nullable field where the source has
//   `x: null` would fire the default in tora but bun keeps
//   null (since spec says undefined-only). The parity holds
//   for non-Nullable struct field shapes (the dominant case).

// Array destr with default — short source falls back.
function f([a = 5, b = 10]: number[]): void {
  console.log(a)
  console.log(b)
}
f([1, 2])                                    // 1 / 2
f([7])                                       // 7 / 10
f([100, 200])                                // 100 / 200

// Array destr default with multi-element source.
function g([a, b = 99, c = 100]: number[]): void {
  console.log(a)
  console.log(b)
  console.log(c)
}
g([1, 2, 3])                                 // 1 / 2 / 3
g([1, 2])                                    // 1 / 2 / 100
g([1])                                       // 1 / 99 / 100

// Object destr with default — non-Nullable field, default is
// dead code.
type O = { x: number, y: number }
function h({ x = 1, y = 2 }: O): void {
  console.log(x)
  console.log(y)
}
h({ x: 10, y: 20 })                          // 10 / 20

// Renamed object destr with default — `{ x: foo = 5 }`.
function i({ x: foo = 5 }: { x: number }): void {
  console.log(foo)
}
i({ x: 99 })                                 // 99

// Regression — destr without defaults still parses + runs.
function j([a, b]: number[]): void {
  console.log(a)
  console.log(b)
}
j([3, 4])                                    // 3 / 4
