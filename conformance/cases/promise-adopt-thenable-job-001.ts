// resolving a promise with another promise runs the inner `then` one
// job later (NewPromiseResolveThenableJob), so the outer settles two
// jobs after the inner would have handed its own reactions the value
const order: string[] = []
const inner = Promise.resolve(5)
new Promise((res) => res(inner)).then(() => order.push("n"))
inner.then(() => Promise.resolve(1)).then(() => order.push("h"))
async function g() { await null; return inner }
g().then(() => order.push("g"))
const pending = new Promise<number>((res) => { Promise.resolve().then(() => res(7)) })
Promise.resolve(0).then(() => pending).then((v) => order.push("p" + v))
Promise.resolve().then(() => order.push("b1")).then(() => order.push("b2")).then(() => order.push("b3")).then(() => order.push("b4")).then(() => order.push("b5")).then(() => console.log(order.join(",")))
Promise.race([inner]).then(() => order.push("r"))
Promise.resolve(3).then().then(() => order.push("t"))
