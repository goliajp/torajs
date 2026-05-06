// T-13.a v0.4.0 fix — `symbol[]` type annotation must resolve to
// Array<Type::Symbol>. Previously check.rs's primitive resolver had
// no `symbol` entry, so `let xs: symbol[]` rejected with
// "unknown type `symbol[]`". test262 staging/sm/Symbol/equality.js
// hits this via mixed-flavor symbol arrays + identity comparison.

let s1 = Symbol()
let s2 = Symbol('description')
let s3 = Symbol('description') // distinct fresh handle, same description
let s4 = Symbol.for('Symbol.iterator')
let s5 = Symbol.iterator

let arr: symbol[] = [s1, s2, s3, s4, s5]
console.log(arr.length)

// Identity: every symbol === itself.
for (let i = 0; i < arr.length; i = i + 1) {
  console.log(arr[i] === arr[i])
}

// Distinctness across the five.
let pairs = 0
for (let i = 0; i < arr.length; i = i + 1) {
  for (let j = 0; j < arr.length; j = j + 1) {
    if (i !== j && arr[i] === arr[j]) {
      pairs = pairs + 1
    }
  }
}
console.log(pairs) // 0 — all five are distinct identities

// Object.is on indexed Symbol-array reads inside a loop — covers the
// specific test262 staging/sm/Symbol/equality.js crash. Object.is
// previously rc_dec'd both args unconditionally, freeing the
// well-known singleton (Symbol.iterator) on the first iter →
// SIGSEGV reading freed memory on the second iter. Borrow guard now
// skips the drop when the source expr is Index / Member / Ident.
let truthy = 0
for (let i = 0; i < arr.length; i = i + 1) {
  for (let j = 0; j < arr.length; j = j + 1) {
    if (Object.is(arr[i], arr[j])) {
      truthy = truthy + 1
    }
  }
}
console.log(truthy) // 5 — exactly the diagonal pairs (i===j)
