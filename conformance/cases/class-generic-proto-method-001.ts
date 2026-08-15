// 405-04 knife 1 — a GENERIC class's prototype carries reified method
// faces. The twin-primary rows' adapter IS the recv-first `__cmany_`
// twin, so the face is minted with the static-face encoding
// `(tag 0, twin = adapter)` and every dispatch site routes it through
// the receiver-first channel.

class G<T> { v: T; constructor(x: T) { this.v = x } m() { return this.v } }
const g = new G<number>(6)
const gp: any = (G as any).prototype

// the reified face exists and is callable
console.log(typeof gp.m)

// detached + .call rebind onto an instance
console.log(gp.m.call(g))

// another specialization shares the generic row's face
const h = new G<string>("s")
console.log(gp.m.call(h))

// the prototype itself as receiver — reads through the any lane
console.log(gp.m())

// non-generic class faces must not regress
class N { k = 5; get() { return this.k } }
console.log(typeof (N as any).prototype.get, (N as any).prototype.get.call(new N()))
