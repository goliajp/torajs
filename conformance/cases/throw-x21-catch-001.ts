// After a catch (or a finally that swallows) the next call to a function
// that can throw but returns normally must not look like a throw.
function maybe(n: number): number {
  if (n < 0) throw new Error('neg ' + n)
  return n + 1
}

function swallow(n: number): number {
  let r = 0
  try {
    r = maybe(n)
  } catch (e) {
    r = -100
  }
  return r + maybe(5)
}

function viaFinally(n: number): number {
  let r = 0
  for (;;) {
    try {
      r = maybe(n)
    } finally {
      r += 1
      break
    }
  }
  return r + maybe(1)
}

function rethrow(n: number): number {
  try {
    return maybe(n)
  } catch (e) {
    throw new Error('re ' + (e as Error).message)
  }
}

let s = 0
for (let i = -3; i < 4; i++) {
  s += swallow(i)
  s += viaFinally(i)
  try {
    s += rethrow(i)
  } catch (e) {
    s += maybe(2)
  }
  s += maybe(i + 10)
}
console.log(s)
try {
  maybe(-1)
} catch (e) {
  console.log((e as Error).message)
}
console.log(maybe(0), swallow(-9), viaFinally(-9))
