// A catch whose entry merges values from the try: the copies into the
// join may leave the block between the probe and the catch with no code
// of its own, and x21 must still be 0 when the next call returns
// normally.
// recursive, so that it stays a call of its own
function maybe(x: number): number {
  if (x < 0) throw new Error('n')
  if (x > 1000) return maybe(x - 1000) + 1000
  return x
}

function f(a: number, b: number): number {
  let r = 0
  try {
    r = maybe(a)
    r = maybe(b)
  } catch (e) {
    r = r - 1
  }
  return r + maybe(5)
}

function many(a: number, b: number, c: number): number {
  let x = 0
  let y = 0
  let z = 0
  try {
    x = maybe(a)
    y = maybe(b) + x
    z = maybe(c) + y
  } catch (e) {
    x = x + 100
    y = y + 200
  }
  return x + y + z + maybe(1)
}

function nested(a: number, b: number): number {
  let r = 0
  let s = 0
  try {
    r = maybe(a)
    try {
      s = maybe(b)
      r = r + s
    } catch (e) {
      s = s - 1
    }
    r = r + maybe(s < 0 ? -s : s)
  } catch (e) {
    r = r - 10
  }
  return r + s + maybe(2)
}

function loopAfter(a: number, n: number): number {
  let r = 0
  try {
    r = maybe(a)
    r = maybe(a - 3)
  } catch (e) {
    r = r + 7
  }
  let sum = 0
  for (let i = 0; i < n; i++) {
    sum += maybe(i) + r
  }
  return sum
}

function leaves(a: number): number {
  let r = 0
  try {
    r = maybe(a)
    r = maybe(-a)
  } catch (e) {
    r = r * 2
  }
  return r
}

function outer(a: number): number {
  // a normal return of `leaves` after it swallowed a throw
  return leaves(a) + maybe(3)
}

console.log(f(1, -1))
console.log(f(2, 3))
console.log(many(1, 2, -3))
console.log(many(1, -2, 3))
console.log(many(4, 5, 6))
console.log(nested(1, -2))
console.log(nested(-1, 2))
console.log(nested(3, 4))
console.log(loopAfter(5, 4))
console.log(loopAfter(1, 3))
console.log(outer(4))
console.log(outer(-4))
