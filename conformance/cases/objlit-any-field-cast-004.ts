// An `any` member of an object literal whose declared type is an
// object layout, holding a struct or class cell of another layout: the
// member takes the cell as it is (a class instance keeps its identity
// and its class); a dynamic object of the layout's fields is copied.
class W {
  a: number
  b: number
  constructor(n: number) {
    this.a = n
    this.b = n + 1
  }
}
class X {
  tag = "x"
}
type S = { k: W; n: number }
type T = { y: number }
type Q = { t: T }
function tryit(f: () => string): string {
  try {
    return f()
  } catch (e) {
    return (e as Error).constructor.name
  }
}
const vals: any[] = [new X(), { z: 1 }, new W(3)]
const out: string[] = []
for (const v of vals) {
  out.push(tryit(() => {
    const s: S = { k: v, n: 1 }
    const back: any = s.k
    return String(back === v) + ":" + String(back instanceof W) + ":" + String(back instanceof X)
  }))
}
console.log(out.join(" "))
const j: any = JSON.parse('{"a":5,"b":6}')
const sj: S = { k: j, n: 2 }
console.log(sj.k.a + sj.k.b)
const wide: any = { y: 2.5, extra: true }
const q: Q = { t: wide }
console.log(q.t.y)
