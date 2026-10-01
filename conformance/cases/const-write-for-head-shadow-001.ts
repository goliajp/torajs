// A `for` head's `let` owns the writes in its test, step and body: a
// same-named top-level `const`, declared before or after the
// function, is not the binding being stepped.
const j = 7
function sumTo(n: number): number {
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    s = s + i
  }
  return s
}
function countDown(n: number): number {
  let steps = 0
  for (let j = n, k = 0; j > k; j--) {
    steps++
  }
  return steps
}
const twice = (n: number): number => {
  let t = 0
  for (let i = 0; i < n; i++) {
    t += 2
  }
  return t
}
const i = parseInt("5", 10)
const k = 9
console.log(sumTo(4), countDown(3), twice(3), i, j, k)
