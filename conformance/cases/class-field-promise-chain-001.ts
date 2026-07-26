// Chaining off a `Promise` held in a field. The promise-chain lane
// decides whether a receiver IS a built-in promise, and it answered for
// a binding by asking that binding's slot — but had no arm at all for a
// field read. So `const p = C.make(); p.then(cb)` chained while
// `c.p.then(cb)` fell through to "unsupported member call shape: then".
//
// A `Promise`-typed field holds the same thing a `Promise`-typed binding
// holds, so it is the same question, asked of the field's type.

class Held {
  p: Promise<number> = Promise.resolve(7)
}
const h = new Held()
h.p.then((v) => {
  console.log("then", v)
})

// A fresh receiver, unbound.
new Held().p.then((v) => {
  console.log("fresh", v + 1)
})

// Awaited rather than chained.
class Awaited {
  p: Promise<number> = Promise.resolve(3)
}
async function viaAwait() {
  const v = await new Awaited().p
  console.log("await", v)
}
viaAwait()

// A non-number value type.
class Strs {
  p: Promise<string> = Promise.resolve("hi")
}
new Strs().p.then((s) => {
  console.log("str", s, s.length)
})

// Assigned by the constructor rather than a field initializer.
class ByCtor {
  p: Promise<number>
  constructor(n: number) {
    this.p = Promise.resolve(n)
  }
}
new ByCtor(9).p.then((v) => {
  console.log("ctor", v)
})

// A field of a nested class instance.
class Leaf {
  p: Promise<number> = Promise.resolve(4)
}
class Holder {
  leaf: Leaf = new Leaf()
}
new Holder().leaf.p.then((v) => {
  console.log("nested", v)
})

// `.catch` off a field, and `.then` chained twice.
class Rejects {
  p: Promise<string> = Promise.reject("boom")
}
new Rejects().p.catch((e) => {
  console.log("caught", e)
})

class Chained {
  p: Promise<number> = Promise.resolve(1)
}
new Chained().p
  .then((v) => v + 10)
  .then((v) => {
    console.log("chained", v)
  })

// An object-literal receiver took this lane already and must keep
// working.
const lit = { p: Promise.resolve(5) }
lit.p.then((v) => {
  console.log("objlit", v)
})

// A plain binding — the shape that always worked.
const bound = Promise.resolve(6)
bound.then((v) => {
  console.log("bound", v)
})
