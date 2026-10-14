// `i % p` in a loop over a counter `i` stepped by one, with `p` the
// loop never writes, reads the same as the remainder it replaces for
// every start, divisor and shape of loop: a negative start (with the
// `-0` of its negative multiples), a divisor of zero, a negative or a
// fractional divisor, a divisor written inside the loop, a step of
// two, an outer loop that does not reset the counter, `continue`, a
// conditional step, a read after the step, constant divisors.
const seed = Number(process.argv[2] ?? '7')
const pool = parseInt(String(seed + 3), 10)
const zero = parseInt(String(seed - 7), 10)
const neg = parseInt(String(-seed + 4), 10)
const start = parseInt(String(seed), 10)

function show(r: number): string {
  return Object.is(r, -0) ? '-0' : String(r)
}

function rems(from: number, to: number, p: number): string {
  const out: string[] = []
  for (let i = from; i < to; i = i + 1) out.push(show(i % p))
  return out.join(' ')
}

// from 0, from a runtime start, from a negative start
console.log(rems(0, 25, pool))
console.log(rems(start, start + 25, pool))
console.log(rems(-12, 13, pool))
// a zero, a negative, a fractional, a one, a huge divisor
console.log(rems(0, 6, zero), rems(-3, 3, zero))
console.log(rems(0, 12, neg), rems(-6, 6, neg))
console.log(rems(0, 8, pool / 4), rems(0, 8, 2.5))
console.log(rems(0, 5, 1), rems(0, 5, 2 ** 53), rems(0, 3, 2 ** 60))

// an index sink over an any[] and a number[]
const anys: any[] = []
const nums: number[] = []
for (let j = 0; j < pool; j = j + 1) {
  anys.push(j + 0.5)
  nums.push(j * 3)
}
let ta = 0
let tn = 0
for (let i = 0; i < 1000; i = i + 1) {
  const x = anys[i % pool]
  ta = ta + x
  tn = tn + nums[i % pool]
}
console.log(ta, tn)

// the divisor written inside the loop
let p = pool
let tw = 0
for (let i = 0; i < 40; i = i + 1) {
  tw = tw * 3 + (i % p)
  tw = tw % 1000003
  if (i === 17) p = p + 2
}
console.log(tw)

// a step of two, and a step of one taken only on some trips
const st: string[] = []
for (let i = 0; i < 30; i = i + 2) st.push(show(i % pool))
function condStep(q: number, stop: number, limit: number): string {
  const seen: string[] = []
  let k = 0
  let trips = 0
  while (k < stop && trips < limit) {
    seen.push(show(k % q))
    if (trips % 3 !== 0) k = k + 1
    trips = trips + 1
  }
  return seen.join(' ')
}
console.log(st.join(' '), '|', condStep(pool, start * 4, start * 14))

// an outer loop around a counter it never resets
let c = 0
let tc = 0
for (let o = 1; o <= 4; o = o + 1) {
  while (c < o * 7) {
    tc = tc * 7 + (c % pool)
    tc = tc % 1000003
    c = c + 1
  }
}
console.log(c, tc)

// continue, a read after the step, a do-while
const sc: string[] = []
for (let i = 0; i < 20; i = i + 1) {
  if (i % 4 === 1) continue
  sc.push(show(i % pool))
}
let d = 0
const sa: string[] = []
while (d < 15) {
  d = d + 1
  sa.push(show(d % pool))
}
let e = start
do {
  sa.push(show(e % pool))
  e = e + 1
} while (e < start + 9)
console.log(sc.join(' '), '|', sa.join(' '))

// constant divisors, and the remainder compared with zero
let hits = 0
let sum3 = 0
for (let i = 0; i < 100; i = i + 1) {
  if (i % 3 === 0) hits = hits + 1
  sum3 = sum3 + (i % 7) * (i % 6) + (i % 8)
}
console.log(hits, sum3)

// a divisor that is a parameter, called with every kind of number
function sumRem(n: number, q: number): string {
  let s = 0
  let last = 0
  for (let i = 0; i < n; i = i + 1) {
    last = i % q
    s = s + last
  }
  return show(s) + '/' + show(last)
}
console.log([pool, 1, zero, neg, 2.5, -0, Infinity, NaN].map((q) => sumRem(12, q)).join(' '))
