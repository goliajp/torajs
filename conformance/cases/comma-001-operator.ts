// V3-18 m1.h.6 — JS spec §13.16 comma operator. Inside parens,
// `(a, b, c)` evaluates each subexpression left-to-right (side
// effects), then returns the rightmost value. Common test262
// pattern + foundational JS expression idiom.
let r = (1, 2, 3)
console.log(r)

let s = ("a", "b")
console.log(s)

let counter = 0
let v = (counter = counter + 1, counter = counter + 1, counter)
console.log(v)
console.log(counter)
