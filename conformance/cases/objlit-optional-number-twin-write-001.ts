// A method's receiver-polymorphic twin writes `this.n = v` with `v`
// retyped `any`: a null, a string or an undefined the any lane hands it
// lands in the optional `number` member's sidecar or sentinel, and the
// typed reads see it.
class K {
  n?: number
  m: number = 1
  set(v: number): void {
    this.n = v
  }
  bump(): void {
    this.n = (this.n ?? 0) + this.m
  }
  get(): any {
    return this.n
  }
}

const k = new K()
const ka: any = k
ka.set(null)
console.log(k.n, typeof k.n, k.n ?? 9, ka.get(), JSON.stringify(k), "n" in k)
ka.set("s")
console.log(k.n, typeof k.n, k.get())
ka.set(undefined)
console.log(k.n, typeof k.n, k.get(), "n" in k)
k.set(3)
console.log(k.n, typeof k.n)
k.bump()
console.log(k.n, Object.values(k).join(","))
const ks: K[] = [k, new K()]
console.log(ks.map((o) => o.n).join("|"), ks.map((o) => typeof o.n).join("|"))
