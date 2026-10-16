// A pending throw is handed to one catch only. Here a callee swallows
// a throw that is pending when it is called, and a call that returns
// normally after that must not pass the same Error on a second time.
// If the call into `g` is ever left without a check that it threw, the
// throw reaches `swallow` instead of the outer catch; either way no
// catch may see the Error `swallow` already took.
const a: number[] = [1, 2]
Reflect.set(a, 'NaN', {
  valueOf() {
    throw new Error('x')
  },
})
let saved: unknown = undefined
// recursive, so that each stays a call of its own
function g(xs: number[], k: number): number {
  if (k > 1000) return g(xs, k - 1000)
  return xs[k] * 2
}
function sig(n: number): number {
  if (n < 0) throw new Error('neg')
  if (n > 1000) return sig(n - 1000) + 1000
  return n + 1
}
function inner(n: number): number {
  if (n < 0) throw new Error('inner')
  return n
}
const fv: (n: number) => number = inner
function swallow(n: number): number {
  if (n > 1000) return swallow(n - 1000)
  let acc = 0
  for (let i = 0; i < n; i++) acc = (acc * 31 + i) | 0
  for (let i = 0; i < n; i++) acc = (acc ^ (i << 3)) | 0
  try {
    return fv(n) + acc
  } catch (e) {
    saved = e
    return -1
  }
}
// its address is taken, so it puts back the x21 it was called with;
// called only with a fraction, so no integer copy of it is made
const sw: (n: number) => number = swallow
function ev(k: number, d: number): number {
  if (d === 0) return 0
  const x = g(a, k)
  const y = swallow(d + 0.5)
  const z = sig(d)
  return x + y + z + ev(k, d - 1)
}
let twice = false
try {
  console.log(sw(1.5))
  console.log(ev(1, 2))
  ev(NaN, 2)
} catch (e) {
  twice = e === saved
}
console.log(twice)
