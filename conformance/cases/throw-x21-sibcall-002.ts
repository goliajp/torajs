// Tail calls between functions that cannot be inlined: one that hands a
// throw back in a register leaving through one that does not, and the
// reverse. The throw reaches the catch, and the caller's own values
// survive it.
function k(n: number): number {
  if (n > 5) throw new Error('k' + n)
  if (n < -100) return k(n + 1) + 1
  return n + 1
}
const kref = k

function g(n: number): number {
  if (n > 6) throw new Error('g' + n)
  if (n < -100) return g(n + 1) + 1
  return n * 2
}

function f(n: number, d: number): number {
  if (n === -5) throw new Error('f')
  if (d > 0) return f(n, d - 1) + 1
  return k(n)
}

function t(n: number, d: number): number {
  if (d > 0) return t(n, d - 1) + 1
  return g(n)
}
const tref = t

function viaF(rounds: number): number {
  let s = 0
  for (let i = 0; i < rounds; i++) {
    try {
      s += f(i, 2)
    } catch (e) {
      s += 1000
    }
    try {
      s += f(i, 0)
    } catch (e) {
      s += 2000
    }
  }
  return s
}

function viaT(rounds: number): number {
  let a = 3
  let b = 5
  let c = 7
  let d = 11
  let e = 13
  let s = 0
  for (let i = 0; i < rounds; i++) {
    try {
      s += t(i, 1)
    } catch (err) {
      s += a + b * 2 + c * 3 + d * 4 + e * 5
    }
    try {
      s += t(i, 0)
    } catch (err) {
      s += a * 5 + b * 4 + c * 3 + d * 2 + e
    }
    a = (a * 3 + i) % 101
    b = (b * 5 + i) % 103
    c = (c + a) % 107
    d = (d + b) % 109
    e = (e + c + d) % 113
  }
  return s * 1000 + a + b + c + d + e
}

console.log(viaF(10))
console.log(viaT(10))
console.log(kref(1), tref(1, 2))
