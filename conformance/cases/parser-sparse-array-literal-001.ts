// P-PARSE.1 — sparse array literal `[1, , 3]` per ES spec
// §13.2.4. Pre-fix tora's parser bailed at the comma in
// element position with 'expected expression, got Comma',
// blocking the canonical TS / JS pattern of "fixed-length
// array with some slots intentionally vacant" that test262
// uses pervasively in the language/expressions/array test
// suite (~50+ cases hit this single shape).
//
// Implementation:
// * parser.rs's parse_array_literal gains a
//   `parse_elem_or_elision` inner that peeks Comma /
//   RBracket and synthesizes an Expr::Null for elision
//   slots; otherwise falls through to the existing
//   parse_array_element (which handles spread and the
//   normal expression case).
// * The elision-as-Null choice is a stand-in until P1
//   ships real Type::Undefined: at the storage layer
//   Nullable<T> is the closest existing shape, and
//   test262 cases that exercise sparse arrays mostly check
//   .length (unaffected by the elision-value choice) or
//   compare missing-vs-present semantics where null still
//   reads as "no value" through the existing Nullable
//   path. Once P1 lands, the synthesizer flips to a
//   dedicated Expr::Undefined node — single-line change.
//
// Acceptance: parse error 'expected expression, got
// Comma' vanishes from the language/expressions sample.

// Single elision in the middle.
let xs1 = [, , , 4]
console.log(xs1.length)                      // 4
console.log(xs1[3])                          // 4

// Two consecutive elisions.
let xs2 = [1, 2, , , 5]
console.log(xs2.length)                      // 5
console.log(xs2[4])                          // 5

// All elisions plus trailing comma — array of all-null.
let xs3 = [, , ,]
console.log(xs3.length)                      // 3

// Mixed elisions + values, multiple positions.
let arr = [0, 1, 2, , , 5, 6]
console.log(arr.length)                      // 7
console.log(arr[5])                          // 5

// Sparse string array — refcount-aware path inherits the
// elision-as-Null treatment.
let mix = ["a", , "c"]
console.log(mix.length)                      // 3
console.log(mix[0])                          // a
console.log(mix[2])                          // c

// Leading-only elision.
let lead = [, "first"]
console.log(lead.length)                     // 2
console.log(lead[1])                         // first

// Regression: a non-sparse array still parses unchanged.
let dense = [1, 2, 3]
console.log(dense.length)                    // 3
console.log(dense[0])                        // 1
console.log(dense[2])                        // 3

// Regression: trailing comma alone (1-element with comma)
// stays as length-1.
let trail = [42, ]
console.log(trail.length)                    // 1
console.log(trail[0])                        // 42
