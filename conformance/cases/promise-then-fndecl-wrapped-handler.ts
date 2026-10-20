// Handlers of a chain over a `Promise` static that keep their closure
// cell: a defaulted parameter, an optional second parameter, an async
// declaration, a body reading `this` / `arguments` / `new.target`, a
// declaration the program assigns over, either handler of a
// two-handler call, and a `.finally` handler.
function withDefault(v: number = 5): number { return v + 10 }
async function asyncAdd(v: number): Promise<number> { return v + 1 }
function readsThis(v: number): string { return (this === undefined) + " " + v }
function readsArgs(v: number): number { return arguments.length + v }
function readsNewTarget(v: number): string { return (new.target === undefined) + " " + v }
function swapped(v: number): number { return v + 1 }
function other(v: number): number { return v * 2 }
function onFulfilled(v: number): number { return v + 1 }
function onRejected(e: any): number { return -e.message.length }
function onFinally(): void { console.log("finally") }
function onRejectedTagged(e: any, tag?: string): string { return (tag ?? "tag") + " " + e.message }

console.log(await Promise.resolve(1).then(withDefault))
console.log(await Promise.resolve(3).then(asyncAdd))
console.log(await Promise.resolve(4).then(readsThis))
console.log(await Promise.resolve(5).then(readsArgs))
console.log(await Promise.resolve(6).then(readsNewTarget))
console.log(await Promise.reject(new Error("e")).then(onFulfilled, onRejected))
console.log(await Promise.resolve(7).then(onFulfilled, onRejected))
console.log(await Promise.reject(new Error("c")).catch(onRejectedTagged))
console.log(await Promise.resolve(11).finally(onFinally))

swapped = other
console.log(await Promise.resolve(8).then(swapped))

// a typed receiver, and a chain whose first link is a two-handler call
const ps: Promise<number>[] = [Promise.resolve(9)]
console.log(await ps[0].then(onFulfilled))
console.log(await Promise.resolve(10).then(onFulfilled, onRejected).then(onFulfilled))
