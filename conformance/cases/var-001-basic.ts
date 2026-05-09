// V3-18 m4 first wedge — `var` keyword lexes as `let` so basic
// non-hoisted usages parse + behave correctly. Full hoisting +
// function-scope semantics (vs let/const block-scope) is a m4.b
// follow-up. Programs that depend on hoisting to use `var` before
// its decl will continue to fail until that ships.

var x = 5
var y = 10
console.log(x + y)

var s = "hello"
console.log(s)

var arr: number[] = [1, 2, 3]
console.log(arr.length)

var b = true
console.log(b)
