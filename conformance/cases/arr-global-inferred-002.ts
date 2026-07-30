// Nested all-literal Array inits promote too — the synthesized
// spelling recurses ('[[1, 2], [3]]' -> number[][], the test262
// matrix prelude shape), and number/f64 elements unify to the wide
// slot at any shared depth.
var m = [[1, 2], [3]];
function g(): number { return m[0][1] + m[1][0] }
console.log(g())
console.log(m.length, m[1].length)

var wide = [[1, 2.5], [3]];
function h(): number { return wide[0][1] + wide[1][0] }
console.log(h())

const deep = [[["a"], ["b"]], [["c"]]];
function pick(): string { return deep[1][0][0] }
console.log(pick())

// inner empty stays main-local (no certain elem type at depth) —
// top-level reads still work
var holey = [[], [1]];
console.log(holey.length)
