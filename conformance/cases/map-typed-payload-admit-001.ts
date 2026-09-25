// Map<K, V> carries its type arguments in the checker: every legal TS
// spelling that moves a Map between differently-annotated places must
// still type-check, and the values read back are unchanged

const a: Map<string, number> = new Map()
a.set("x", 1).set("y", 2)
console.log(a.size, a.get("x"), a.get("y"))

function total(m: Map<string, number>): number {
  let t = 0
  m.forEach((v: number, k: string, mm: Map<string, number>) => {
    t = t + v + (mm.has(k) ? 1 : 0)
  })
  return t
}
console.log(total(a), total(new Map()))

function firstKey<T>(m: Map<string, T>): string {
  for (const k of m.keys()) {
    return k
  }
  return "none"
}
console.log(firstKey(a), firstKey(new Map<string, boolean>()))

function loose(m: Map<string, any>): void {
  m.set("z", "text")
}
const b = new Map<string, number>()
b.set("n", 5)
loose(b)
console.log(b.get("n"), b.get("z"), b.size)

class Reg {
  m: Map<number, string> = new Map()
  add(k: number, v: string): Reg {
    this.m.set(k, v)
    return this
  }
}
const r = new Reg().add(1, "one").add(2, "two")
console.log(r.m.get(2), r.m.size)

function make(): Map<string, Array<number>> {
  const m = new Map<string, Array<number>>()
  m.set("p", [1, 2, 3])
  return m
}
console.log(make().get("p")!.length)
