// §10.1.9.2 OrdinarySet consults the [[Prototype]] chain on an own
// miss, and a receiver that was never re-parented still HAS one. The
// write side used to follow only explicit links, so an accessor a
// program installed on %Object.prototype% was unreachable from every
// ordinary object: the assignment minted a fresh own key instead.

let seen: any = null
Object.defineProperty(Object.prototype, "acc", {
  set(v: any) {
    seen = [this, v]
  },
  configurable: true,
})
Object.defineProperty(Object.prototype, "ro", { value: 1, configurable: true })
Object.defineProperty(Object.prototype, "rw", {
  value: 1,
  writable: true,
  configurable: true,
})

// an inherited accessor runs, with the ORIGINAL receiver as `this`
const o: any = { own: 1 }
o.acc = 9
console.log(seen[0] === o, seen[1], Object.getOwnPropertyNames(o).join(","))

// an inherited non-writable data property rejects the strict assign
try {
  o.ro = 2
  console.log("ro no-throw")
} catch (e: any) {
  console.log("ro", e.constructor.name)
}

// a WRITABLE inherited data property does not: the write creates an
// own property on the receiver and leaves the prototype's alone
o.rw = 2
console.log(o.rw, (Object.prototype as any).rw)

// a struct receiver's chain runs past its class prototype to the root
class C {
  x = 1
}
const c: any = new C()
c.acc = 8
console.log(seen[0] === c, seen[1])

// a function link with no expando table yet is a link, not the end
function g() {}
const viaFn: any = Object.create(g)
viaFn.acc = 7
console.log(seen[0] === viaFn, seen[1])

// an explicit null [[Prototype]] has no chain to consult
const bare: any = Object.create(null)
bare.acc = 6
console.log(bare.acc, seen[1])

// %Object.prototype% is the root: its own write stays ordinary
;(Object.prototype as any).installed = 5
console.log(({} as any).installed)
