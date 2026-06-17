// `Array(...)` without `new` — spec-equivalent to `new Array(...)`
// (ES §23.1.1.1). Desugar rewrites Call → New so the existing path covers it.
console.log(Array(3).fill(0))
console.log(Array(3).fill('x'))
console.log(Array().length)
console.log(Array(1, 2, 3))
console.log(Array(0).length)

// Mix with `new Array(...)`
console.log(new Array(2).fill('y'))
console.log(Array(2).length === new Array(2).length)

// (Array<Any>.fill().map(...) chain is a separate Array<Any>-elem
// dispatch substrate item; not covered here.)
