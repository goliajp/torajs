// an async function that returns a promise without suspending settles
// a fresh promise of its own that adopts the returned one
const order: string[] = []
const inner = Promise.resolve(5)
async function f() { return inner }
const pf = f()
console.log(pf === inner)
pf.then((v) => order.push("f" + v))
async function bad(): Promise<number> { return Promise.reject(new Error("no")) }
bad().catch((e) => order.push("e:" + (e as Error).message))
async function th(): Promise<any> { return { then(r: any) { order.push("t"); r(9) } } }
th().then((v) => order.push("th" + v))
Promise.resolve().then(() => order.push("b1")).then(() => order.push("b2")).then(() => order.push("b3")).then(() => order.push("b4")).then(() => console.log(order.join(",")))
