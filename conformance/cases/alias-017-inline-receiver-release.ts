// A receiver or argument read straight out of a field, an element or
// a reassigned top-level binding stays alive for the whole call, even
// when the callee overwrites the place it came from: every read of
// `this` or the parameter after the overwrite still sees the old
// object, never the one allocated next.
class Q {
  a: number
  b: number
  constructor(a: number, b: number) {
    this.a = a
    this.b = b
  }
}

class P {
  name: string
  pos: number
  constructor(pos: number) {
    this.name = "p"
    this.pos = pos
  }
  run(): number {
    box.p = null
    box.q = new Q(1, 900)
    return this.pos
  }
  get at(): number {
    box.p = null
    box.q = new Q(2, 901)
    return this.pos
  }
  set at(v: number) {
    box.p = null
    box.q = new Q(3, 902)
    this.pos = this.pos + v
    console.log("set", this.pos)
  }
}

class Box {
  p: P | null = null
  q: Q | null = null
}

const box = new Box()

// a field read as the receiver of a method call
for (let i = 0; i < 3; i = i + 1) {
  box.p = new P(7 + i)
  console.log("pos", box.p!.run())
}

// a field read as a plain argument
function readAfterClear(p: P): number {
  box.p = null
  box.q = new Q(4, 903)
  return p.pos
}
for (let i = 0; i < 3; i = i + 1) {
  box.p = new P(20 + i)
  console.log("arg", readAfterClear(box.p!))
}

// getter and setter on a field-read receiver
box.p = new P(30)
console.log("get", box.p!.at)
box.p = new P(40)
box.p!.at = 5

// a reassigned top-level binding as receiver and argument
let g: P = new P(50)
function viaGlobal(p: P): number {
  g = new P(1)
  box.q = new Q(6, 905)
  return p.pos
}
console.log("global-arg", viaGlobal(g))
class T {
  n: number
  constructor(n: number) {
    this.n = n
  }
  go(): number {
    t = new T(0)
    box.q = new Q(8, 907)
    return this.n
  }
}
let t: T = new T(80)
console.log("global-recv", t.go(), t.n)

// an element read as the receiver and as an argument
const ps: P[] = [new P(90), new P(91)]
class W {
  n: number
  constructor(n: number) {
    this.n = n
  }
  clear(): number {
    ws[0] = new W(0)
    box.q = new Q(9, 908)
    return this.n
  }
}
const ws: W[] = [new W(100)]
console.log("elem-recv", ws[0].clear())
function dropFirst(p: P): number {
  ps[0] = new P(0)
  box.q = new Q(10, 909)
  return p.pos
}
console.log("elem-arg", dropFirst(ps[0]))

// an overriding method reached through the vtable
class Base {
  v: number
  constructor(v: number) {
    this.v = v
  }
  step(): number {
    return this.v
  }
}
class Leaf extends Base {
  step(): number {
    holder.b = null
    box.q = new Q(11, 910)
    return this.v + 1
  }
}
class Holder {
  b: Base | null = null
}
const holder = new Holder()
holder.b = new Leaf(110)
console.log("vtable", holder.b!.step())
holder.b = new Base(120)
console.log("vtable-base", holder.b!.step())

// a setter whose assignment value is used, read out of the place the
// setter clears
class SV {
  v = 1
}
class SBox {
  p: SV | null = new SV()
}
const sbox = new SBox()
class SC {
  set s(x: SV | null) {
    sbox.p = null
  }
}
const sc = new SC()
const sr = (sc.s = sbox.p)
new SV()
console.log("set-value", sr!.v)
