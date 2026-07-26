// `typeof` on a field of a type that spells `undefined` with the
// generic immortal cell must consult the cell, not the slot's static
// type. The class factory seeds such a field with that cell, so an
// untouched field IS `undefined` (ES §13.5.3 / §6.1.1.1) — and the two
// questions used to disagree: `c.d === undefined` answered true while
// `typeof c.d` answered "object".
//
// A field holding a LIVE value must keep answering the live name; the
// runtime branch compares the cell's address, so the hit path is
// unchanged.

class Unset {
  d: Date
  r: RegExp
  b: bigint
  y: symbol
  p: Promise<number>
}

const u = new Unset()
console.log(typeof u.d, typeof u.r, typeof u.b, typeof u.y, typeof u.p)
console.log(u.d === undefined, u.r === undefined, u.b === undefined)
console.log(typeof u.d === "undefined", typeof u.b === "undefined")

// A live value in the same slot answers its own name.
class Live {
  d: Date = new Date(0)
  r: RegExp = /ab+c/
  b: bigint = 7n
  y: symbol = Symbol("s")
}

const l = new Live()
console.log(typeof l.d, typeof l.r, typeof l.b, typeof l.y)
console.log(l.d === undefined, l.b === undefined)

// Written by the constructor rather than a field initializer.
class Ctor {
  d: Date
  b: bigint
  constructor() {
    this.d = new Date(0)
    this.b = 3n
  }
}

const c = new Ctor()
console.log(typeof c.d, typeof c.b)

// Written after construction — the slot stops being the cell.
const w = new Unset()
console.log(typeof w.d)
w.d = new Date(0)
console.log(typeof w.d)

// A nested class field: the inner object is seeded the same way.
class Inner {
  d: Date
}
class Outer {
  i: Inner = new Inner()
}
const o = new Outer()
console.log(typeof o.i, typeof o.i.d)

// An object literal always initializes its fields, so those stay live.
const lit = { d: new Date(0), b: 5n }
console.log(typeof lit.d, typeof lit.b)

// Reading a field in a loop: the fresh receiver each turn is the cell.
let seen = 0
for (let i = 0; i < 3; i++) {
  const fresh = new Unset()
  if (typeof fresh.d === "undefined") seen++
}
console.log(seen)

// Initialized number / string / boolean fields are not part of this
// family and must keep their static fold — the widened gate must not
// reach them. (An UNINITIALIZED one of these still answers its typed
// zero where the language answers `undefined`; that is the deeper
// fabricated-seed axis, tracked separately.)
class Plain {
  n: number = 0
  s: string = ""
  t: boolean = false
}
const pl = new Plain()
console.log(typeof pl.n, typeof pl.s, typeof pl.t)
console.log(pl.n, JSON.stringify(pl.s), pl.t)
