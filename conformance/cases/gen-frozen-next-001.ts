// a generator object's state lives in internal slots, which freezing
// or sealing the object does not reach: next() keeps advancing

function* g() {
  yield 1
  yield 2
}
const it = g()
Object.freeze(it)
console.log(it.next().value, it.next().value, it.next().done)

function* count(n: number) {
  let i = 0
  while (i < n) {
    yield i
    i = i + 1
  }
}
const c = count(4)
Object.seal(c)
let s = 0
for (const v of c) s = s + v
console.log(s, Object.isFrozen(it), Object.isSealed(c))

// the user's own `this` writes inside a generator method still meet
// the freeze
class K {
  x = 0
  n = 0;
  *step() {
    this.x = 1
    yield this.x
  }
  *bump() {
    this.n++
    yield this.n
  }
}
const k = new K()
Object.freeze(k)
try {
  k.step().next()
  console.log("no throw")
} catch (e) {
  console.log("assign threw", e instanceof TypeError, k.x)
}
try {
  k.bump().next()
  console.log("no throw")
} catch (e) {
  console.log("incr threw", e instanceof TypeError, k.n)
}

