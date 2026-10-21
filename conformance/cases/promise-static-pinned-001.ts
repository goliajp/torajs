// a program that never writes Promise.resolve / Promise.reject: every
// mint keeps its value's form for a reader that only knows `any`
function add1(v: number): number { return v + 1 }

function show(v: any): void {
  console.log(typeof v, JSON.stringify(v))
}

const n = parseInt("5", 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  total = total + (await Promise.resolve(i).then(add1))
}
console.log(total)

const p0: any = Promise.resolve()
show(await p0)
const p1: any = Promise.resolve(undefined)
show(await p1)
const p2: any = Promise.resolve(7)
show(await p2)
const p3: any = Promise.resolve(2.5)
show(await p3)
const p4: any = Promise.resolve(true)
show(await p4)
const p5: any = Promise.resolve("str")
show(await p5)
const p6: any = Promise.resolve(null)
show(await p6)
const p7: any = Promise.resolve({ k: 1 })
show(await p7)

const r: any = Promise.reject("no")
r.catch((e: any) => console.log("caught", e))
await Promise.resolve(0)
