// Release chains whose cells sit in the cycle-root buffer: each level
// is buffered when the loop variable moves on to the next one, and
// releasing the head scrubs the levels from the buffer's tail, newest
// first. A scrub that left the emptied tail slots in place walked all
// of them again on the next level, quadratic in the chain length (a
// 100k proxy chain cost 2.5x its linear price, a 200k one 5.7x).
function proxies(n: number): any {
  let p: any = { depth: 0 }
  for (let i = 0; i < n; i++) p = new Proxy(p, {})
  return p
}
function helpers(n: number): any {
  let it: any = [1, 2, 3].values()
  for (let i = 0; i < n; i++) it = it.map((x: number) => x + 1)
  return it
}
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
function arrays(n: number): any[] {
  let a: any[] = []
  for (let i = 0; i < n; i++) a = [a, i]
  return a
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
  let p: any = proxies(100000)
  console.log("proxy", typeof p)
  p = null
  let h: any = helpers(100000)
  console.log("helper", typeof h.next)
  h = null
  let m: Map<string, any> | null = maps(50000)
  console.log("map", m.get("i"), m.get("prev").get("i"))
  m = null
  let a: any[] | null = arrays(50000)
  console.log("array", a[1], a[0][1])
  a = null
  let d: any = dicts(50000)
  console.log("dict", Object.keys(d)[0])
  d = null
}
main()
console.log("done")
