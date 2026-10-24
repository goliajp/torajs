// A generator nested in a generic function reads its creator's type
// parameter; the lifted generator no longer declares it, so the yield
// type stays dynamic instead of naming it.
function wrap<T>(x: T) {
  function* g() {
    yield x
    yield x
  }
  return g()
}
for (const v of wrap(3)) console.log(v, typeof v)
for (const v of wrap("w")) console.log(v, typeof v)

function copy<T>(x: T) {
  function* g() {
    const y = x
    yield y
  }
  return g()
}
for (const v of copy(true)) console.log(v, typeof v)
for (const v of copy([1, 2])) console.log(JSON.stringify(v))

function twice<K>(k: K) {
  const gen = function* (n: number) {
    for (let i = 0; i < n; i++) yield k
  }
  return gen(2)
}
console.log([...twice("p")].join(","))
console.log([...twice(5)].map((v) => typeof v).join(","))
