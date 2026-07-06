// RC-4 F1b-2 — a NULL ptr in a Str-typed slot denotes the JS
// `undefined` value. Two producer shapes: an uncaptured regex
// group slot and an `undefined` element in a string-array literal.
// eq is identity (never content), concat is the text "undefined";
// pre-fix both SIGSEGV'd inside __torajs_str_eq / __torajs_str_concat
// (test262 S15.5.4.10_A2_T6/T10 family).

let m = "ab5".match(/(a)(q)?/)
console.log(m[1])
console.log(m[2] === undefined)
console.log(m[2] !== undefined)
console.log(m[2] === m[2])
console.log("got: " + m[2])

let expected = ["a", undefined]
console.log(m[1] === expected[0])
console.log(m[2] === expected[1])
console.log(m[1] !== expected[1])
