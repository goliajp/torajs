// Higher-order fn + capturing closure. tr lifts every closure to
// a top-level FnDecl with an environment block (refcounted on
// shared captures so multi-closure sharing is safe). Closures can
// be passed to .then for promise chains too.

function makeAdder(x: number): (y: number) => number {
  return function (y: number): number {
    return x + y
  }
}

let add5 = makeAdder(5)
let add10 = makeAdder(10)

console.log(add5(3)) // 8
console.log(add10(7)) // 17

// Capturing closure as a Promise.then callback (T-15.g.5).
let multiplier = 100
let p = Promise.resolve(7).then(function (v: number): number {
  return v * multiplier
})
console.log(await p) // 700
