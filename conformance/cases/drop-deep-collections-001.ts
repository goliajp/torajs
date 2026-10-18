// Deep chains through the runtime's container cells: a Map whose value
// is the previous Map, a Set holding the previous Set, and a dictionary
// object whose computed key holds the previous one. Releasing the head
// releases each level in turn; the native depth must stay bounded.
// Building each chain costs more than releasing it, so every shape is
// built once.
function maps(n: number): Map<string, any> {
  let m = new Map<string, any>()
  for (let i = 0; i < n; i++) {
    const x = new Map<string, any>()
    x.set("prev", m)
    x.set("i", i)
    m = x
  }
  return m
}
function sets(n: number): Set<any> {
  let s = new Set<any>()
  for (let i = 0; i < n; i++) {
    const x = new Set<any>()
    x.add(s)
    x.add(i)
    s = x
  }
  return s
}
function dicts(n: number): any {
  let h: any = {}
  for (let i = 0; i < n; i++) {
    const o: any = {}
    o["k" + (i % 3)] = h
    h = o
  }
  return h
}

function main(): void {
  let m: Map<string, any> | null = maps(200000)
  console.log("map", m.get("i"), m.get("prev").get("i"))
  m = null
  let s: Set<any> | null = sets(200000)
  console.log("set", s.size)
  s = null
  let d: any = dicts(200000)
  console.log("dict", Object.keys(d)[0])
  d = null
}
main()
console.log("done")
