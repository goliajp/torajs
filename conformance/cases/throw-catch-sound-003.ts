// `p instanceof Array` walks `p`'s prototype chain (§7.3.22
// OrdinaryHasInstance), and a Proxy's `getPrototypeOf` trap runs on
// the way and can throw. The try body's only `throw` is of a number,
// yet the catch receives the trap's TypeError: a catch that reads its
// parameter as a number sees a value that is not the number thrown.

function viaProxy(p: any, n: number): number {
  try {
    if (n < 0) throw n
    const r = p instanceof Array
    return r ? 1 : 0
  } catch (e) {
    return (e as number) === -1 ? 10 : 20
  }
}

const trap = new Proxy({}, {
  getPrototypeOf() {
    throw new TypeError("trap")
  },
})
console.log(viaProxy(trap, 1))
console.log(viaProxy({}, 1))
console.log(viaProxy([], 1))
console.log(viaProxy(trap, -1))
