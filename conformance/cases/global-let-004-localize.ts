// Localize — an annotated primitive top-level `let` that no named-fn
// body references stays a main-fn local (no data-global slot), so the
// slot-promotion family can dissolve it into registers. Semantics must
// be byte-identical to the promoted form: read/write ordering, width
// widening, and closure capture all behave as before.

// localized: only main reads/writes these (collatz / prime_count shape)
let sum: number = 0
let i: number = 0
while (i < 100) {
  sum = sum + i
  i = i + 1
}
console.log(sum)

// localized f64 widen: `: number` annotation + fractional init
let acc: number = 0.5
let k: number = 0
while (k < 4) {
  acc = acc + 0.25
  k = k + 1
}
console.log(acc)

// localized bool flip
let flag: boolean = false
let m: number = 0
while (m < 3) {
  flag = !flag
  m = m + 1
}
console.log(flag)

// control: a named fn references this one — it must keep the
// data-global slot and stay visible from the fn body
let base: number = 7
function addBase(x: number): number {
  return x + base
}
console.log(addBase(5))

// control: named fn WRITES the global, main reads the update back
let counter: number = 0
function bump(): void {
  counter = counter + 1
}
bump()
bump()
console.log(counter)

// localized + closure capture: captures copy through __env from the
// main-fn local — the localized home is exactly what capture expects
let cap: number = 21
const dbl = () => cap * 2
console.log(dbl())

// localized despite a same-named fn PARAM (prime_count shape): the
// param `n` covers isOdd's whole body, so its idents never resolve
// to the top-level `n` — the binding localizes and both stay correct
function isOdd(n: number): boolean {
  return n % 2 === 1
}
let n: number = 0
let odds: number = 0
while (n < 10) {
  if (isOdd(n)) {
    odds = odds + 1
  }
  n = n + 1
}
console.log(odds)
console.log(n)
