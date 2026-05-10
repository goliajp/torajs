// V3-18 m1.h.23 — closure-param inference for top-level lets
// initialized to a literal array. Pre-fix the inference pass
// only walked FnDecl bodies + only handled lets with explicit
// type annotations. So `let arr = [1,2,3]; arr.find(x => ...)`
// failed with "parameter `x` of function `__closure_0` requires
// a type annotation".
//
// Now: walk top-level stmts too, plus infer let init shapes for
// number / string / boolean / array literals.

let arr = [1, 2, 3, 4, 5]
console.log(arr.find((x: number) => x > 3))   // 4 — explicit ann (still works)
console.log(arr.find(x => x > 3))               // 4 — inferred
console.log(arr.findIndex(x => x > 3))          // 3
console.log(arr.findLast(x => x > 0))           // 5
console.log(arr.findLastIndex(x => x > 0))      // 4
console.log(arr.some(x => x > 4))               // true
console.log(arr.every(x => x > 0))              // true
console.log(arr.every(x => x > 2))              // false

let strs = ["a", "bb", "ccc"]
console.log(strs.find(s => s.length > 1))       // bb
console.log(strs.some(s => s === "ccc"))        // true
console.log(strs.map(s => s + "!"))             // [ "a!", "bb!", "ccc!" ]
