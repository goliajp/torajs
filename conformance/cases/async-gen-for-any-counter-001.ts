// an async generator's loop counter starting from an untyped value
// lives in an `any` slot of the machine; `a++` must step it by one
const P = (v: any) => Promise.resolve(v)
class A {
  k = 1
  async *cond(o: any) {
    const log: string[] = []
    for (let a = this.k; a < 2; a++) { log.push("i" + a); yield a; log.push("r" + a) }
    log.push("after:" + log.join(","))
    yield (o != null ? o[await P("p")] : undefined)
    yield log.join(" ")
  }
  async *init() {
    for (let a = this.k, b = await P(a + 1); a < 2; a++) yield a + b
  }
  async *initThenKey() {
    for (let a = this.k, b = await P(a + 1); a < 2; a++) yield a + b
    yield { [await P("q")]: this.k }.q
  }
  async *yieldFirst() {
    yield 5
    for (let a = this.k, b = await P(a + 1); a < 2; a++) yield a + b
  }
  async *optional(o: any) {
    for (let a = this.k, b = await P(a + 1); a < 2; a++) yield a + b
    yield o?.p
    yield (await P(o))?.p
  }
  async *half() {
    for (let a = this.k / 2; a < 3; a++) yield a
  }
}
async function drain(it: any) {
  const out: any[] = []
  let n = 0
  for await (const v of it) { out.push(v); if (++n > 6) { out.push("runaway"); break } }
  return out.join(" ")
}
async function main() {
  const a = new A()
  console.log(await drain(a.cond({ p: 7 })))
  console.log(await drain(a.init()))
  console.log(await drain(a.initThenKey()))
  console.log(await drain(a.yieldFirst()))
  console.log(await drain(a.optional({ p: 7 })))
  console.log(await drain(a.half()))
}
main()
