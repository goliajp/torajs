// An object type written in place with a method (`{ m: () => number }`)
// is as structural as a declared one: an instance of a class with the
// method is one of its values, written into such a slot directly or
// through `any`, and a call through the slot runs the instance's own
// method. A literal of the shape stays one too.
class K4 {
  x = 1
  m(): number {
    return 9 + this.x
  }
}
class K5 {
  m(): number {
    return 50
  }
  other(): string {
    return "o"
  }
}
type HI = { k: { m: () => number } }
class CI {
  k: { m: () => number }
  constructor() {
    this.k = { m: () => 1 }
  }
}
function retI(v: any): { m: () => number } {
  return v
}
function call(s: { m: () => number }): number {
  return s.m()
}
const v: any = new K4()
console.log(retI(v).m())
const h: HI = { k: v }
console.log(h.k.m())
const c = new CI()
console.log(c.k.m())
c.k = v
console.log(c.k.m())
let s: { m: () => number } = { m: () => 2 }
console.log(s.m())
s = v
console.log(s.m())
s = new K5()
console.log(s.m())
console.log(call(new K4()), call(new K5()), call({ m: () => 3 }))
const xs: { m: () => number }[] = [new K4(), new K5(), { m: () => 4 }]
let sum = 0
for (const e of xs) sum += e.m()
console.log(sum)
