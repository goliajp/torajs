// The other side of class-field-store-unguarded-001: each spelling
// below can make a declared field refuse or divert a typed store, and
// the store keeps its guard. `freeze` is reached through a key built
// at run time, so only `Object` held as a value names it.
class Box {
  n: number
  tag: string
  constructor(n: number) {
    this.n = n
    this.tag = "t"
  }
}

const O: any = Object
const a = new Box(1)
O["fre" + "eze"](a)
try {
  a.n = 2
  console.log("no throw", a.n)
} catch (e) {
  console.log("caught", (e as Error).constructor.name, a.n)
}

const c = new Box(4)
Object.defineProperty(c, "n", { value: 9, writable: false })
try {
  c.n = 5
  console.log("no throw", c.n)
} catch (e) {
  console.log("caught", (e as Error).constructor.name, c.n)
}
