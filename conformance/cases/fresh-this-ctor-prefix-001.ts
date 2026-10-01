// The leading `this.f = v` writes of a constructor reach an instance
// nothing else has seen; every way the instance can leave that state
// before a write still has to be refused or released correctly
// (§10.1.9.2): a parent that freezes, a freeze inside the value, a
// closure capturing `this`, and a subclass writing after `super()`.
class Leaf {
  n: number
  constructor(n: number) {
    this.n = n
  }
}
class Frozen {
  constructor() {
    Object.freeze(this)
  }
}
class AfterSuper extends Frozen {
  x: number | null = null
  constructor() {
    super()
    this.x = 1
  }
}
try {
  new AfterSuper()
  console.log("after super no throw")
} catch (e) {
  console.log("after super threw:", e instanceof TypeError)
}
function lockAndCount(o: object, k: number): number {
  Object.freeze(o)
  return k + 1
}
class InValue {
  a: number = 0
  b: number = 0
  constructor(k: number) {
    this.a = k
    this.b = lockAndCount(this, k)
  }
}
try {
  new InValue(5)
  console.log("in value no throw")
} catch (e) {
  console.log("in value threw:", e instanceof TypeError)
}
class Captured {
  cb: (() => number) | null = null
  v: number = 0
  constructor() {
    this.cb = () => {
      Object.freeze(this)
      return 1
    }
    this.v = this.cb()
    this.v = 2
  }
}
try {
  new Captured()
  console.log("captured no throw")
} catch (e) {
  console.log("captured threw:", e instanceof TypeError)
}
class Setter {
  k: number = 0
  set v(n: number) {
    if (n > 1) throw new RangeError("too big")
    this.k = n
  }
  constructor(n: number) {
    this.v = n
  }
}
try {
  console.log("setter small:", new Setter(1).k)
  new Setter(2)
  console.log("setter no throw")
} catch (e) {
  console.log("setter threw:", e instanceof RangeError)
}
function viaCtor(n: number): number {
  return new Setter(n).k
}
try {
  console.log("via ctor:", viaCtor(1), viaCtor(3))
} catch (e) {
  console.log("via ctor threw:", e instanceof RangeError)
}
class Base {
  p: Leaf | null
  q: any
  constructor(p: Leaf | null, q: any) {
    this.p = p
    this.q = q
  }
}
class Extended extends Base {
  r: Leaf | null = null
  constructor(p: Leaf, q: any) {
    super(p, q)
    this.r = new Leaf(99)
    this.p = this.r
  }
}
const r = new Extended(new Leaf(7), "seven")
console.log("extended:", (r.p as Leaf).n, (r.r as Leaf).n, r.q)
const b = new Base(null, undefined)
console.log("base:", b.p === null, b.q === undefined)
let total = 0
for (let i = 0; i < 50000; i = i + 1) {
  const x = new Base(new Leaf(i), i)
  total = (total + (x.p as Leaf).n + x.q) | 0
}
console.log("churn:", total)
