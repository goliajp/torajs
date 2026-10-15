// A field the constructor stores before anything can see the instance
// is never read with the factory's seed in it, so the factory no longer
// builds an empty array, map or struct for it. Each class below covers
// one road around that: a store whose value throws first (the half-built
// instance is dropped), a field initializer and a constructor store of
// the same field, a subclass (the base keeps its seeds), and many
// constructions in a loop.
class Pt {
  x: number
  y: number
  constructor(x: number, y: number) {
    this.x = x
    this.y = y
  }
}

class Holder {
  xs: number[]
  tags: Map<string, number>
  at: Pt
  constructor(xs: number[], n: number) {
    this.xs = xs
    this.tags = new Map()
    this.at = new Pt(n, n + 1)
  }
}

function boom(): number[] {
  throw new Error("boom")
}

class Thrower {
  ys: number[]
  zs: string[]
  constructor() {
    this.ys = boom()
    this.zs = ["never"]
  }
}

class Init {
  xs: number[] = [9]
  constructor(n: number) {
    this.xs = [n, n]
  }
}

class Base {
  xs: number[]
  constructor(xs: number[]) {
    this.xs = xs
  }
}
class Derived extends Base {
  ys: string[]
  constructor() {
    super([7])
    this.ys = ["d"]
  }
}

let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  const h = new Holder([i, 1], i)
  h.tags.set("k", i)
  total = total + h.xs.length + h.at.y + (h.tags.get("k") ?? 0)
}
console.log(total)
try {
  new Thrower()
} catch (e) {
  console.log("caught", (e as Error).message)
}
const ini = new Init(4)
console.log(ini.xs.join(","))
const d = new Derived()
console.log(d.xs.join(","), d.ys.join(","), JSON.stringify(new Holder([2], 3)))
