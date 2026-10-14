// a parameter, var or let named like the iterator method `next` inside
// an async function or a generator is an ordinary binding
async function a(next: number) { await 0; return next + 1 }
async function b() { var next = 3; await 0; next = next * 2; return next }
async function c() { let next = 1; await 0; next++; const f = () => next; return f() }
async function d() { const next = 5; async function* g() { yield 1; yield 2 } const out: number[] = []; for await (const v of g()) out.push(v); return next + ":" + out.join(",") }
async function e(req: string, next: (s: string) => string) { await 0; return next(req) }
function* gen(next: number) { const it: any = { next }; yield it.next; yield next }
async function f(next: number) { const o = { next }; await 0; return Object.keys(o).join(",") + o.next }
async function main() {
  console.log(await a(1), await b(), await c(), await d())
  console.log(await e("r", (s) => s + "!"))
  const g: any = gen(9)
  console.log(g.next().value, g.next().value, g.next().done)
  console.log(await f(4))
}
main()
