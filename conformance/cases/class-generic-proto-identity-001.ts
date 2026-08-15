// 405-04 knife 2 — Object.getPrototypeOf on a GENERIC class instance.
// The instance wears a per-factory specialization tag whose registry
// slots were never filled (proto/class registration runs under the
// class's MAIN tag); the registry read aliases the specialization tag
// to the main slot by name identity, the same verdict instanceof uses.

class G<T> { v: T; constructor(x: T) { this.v = x } m() { return this.v } }
const g = new G<number>(6)
const p: any = Object.getPrototypeOf(g)

// the prototype resolves, with identity
console.log(typeof p)
console.log(p === (G as any).prototype)

// the reified method face is reachable through it (knife 1)
console.log(typeof p.m)
console.log(p.m.call(g))

// the constructor link resolves through the aliased prototype
console.log(g.constructor === (G as any))

// two generic classes never cross-alias
class H<T> { v: T; constructor(x: T) { this.v = x } m() { return "H" } }
const hh = new H<number>(1)
console.log(Object.getPrototypeOf(hh) === (H as any).prototype)
console.log(Object.getPrototypeOf(hh) === (G as any).prototype)

// a second specialization of the same class resolves the same slot
const gs = new G<string>("s")
console.log(Object.getPrototypeOf(gs) === (G as any).prototype)

// non-generic classes must not regress
class N { k = 5 }
console.log(Object.getPrototypeOf(new N()) === (N as any).prototype)
