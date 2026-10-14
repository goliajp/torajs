// A method called through `any` runs its receiver-polymorphic twin,
// whose parameters and locals are all `any`: a write there through a
// local receiver carries whatever the caller passed into an optional
// `number` member, and the typed reads see it.
interface Inner {
  a?: number
  b: number
}
class K {
  inner: Inner | null = null
  set(v: number): void {
    const t = this.inner
    if (t) t.a = v
  }
}

const k = new K()
k.inner = { a: 1, b: 2 }
const ka: any = k
const i = k.inner
ka.set(null)
if (i) console.log(i.a, typeof i.a, i.a ?? 9, JSON.stringify(i))
ka.set("s")
if (i) console.log(i.a, typeof i.a, i.a === "s" as any)
ka.set(undefined)
if (i) console.log(i.a, typeof i.a, "a" in i, JSON.stringify(i))
k.set(4)
if (i) console.log(i.a, typeof i.a)

