// Closures released through deep chains: each closure captures
// the previous one (a `const` capture and a `let` capture), a class
// instance holds a closure that captures the previous instance, and an
// object holds a class instance whose closure captures the previous
// object. Releasing the last one releases each environment in turn;
// the native depth must stay bounded. Every chain is built and dropped
// twice, and a fresh closure runs afterwards.
class K {
  f: () => number
  constructor(f: () => number) { this.f = f }
}
type O = { k: K }

function constChain(n: number): () => number {
  let f: () => number = () => 0
  for (let i = 0; i < n; i++) {
    const g = f
    f = () => g() + 1
  }
  return f
}
function letChain(n: number): () => number {
  let f = () => 0
  for (let i = 0; i < n; i++) {
    let g = f
    f = () => g()
  }
  return f
}
function classChain(n: number): K {
  let k = new K(() => 0)
  for (let i = 0; i < n; i++) {
    const prev = k
    k = new K(() => prev.f() + 1)
  }
  return k
}
function objChain(n: number): O {
  let h: O = { k: new K(() => 0) }
  for (let i = 0; i < n; i++) {
    const p = h
    h = { k: new K(() => p.k.f() + 1) }
  }
  return h
}

for (let round = 0; round < 2; round++) {
  let f = constChain(1000000)
  console.log("const", typeof f)
  f = () => 5
  let g = letChain(1000000)
  console.log("let", typeof g)
  g = () => 6
  let k = classChain(300000)
  console.log("class", typeof k.f)
  k = new K(() => 7)
  let o = objChain(1000000)
  console.log("object", typeof o.k.f)
  o = { k: new K(() => 8) }
  console.log("after", f(), g(), k.f(), o.k.f())
}
console.log("done")
