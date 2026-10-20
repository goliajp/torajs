// A top-level function handed to `.then` / `.catch` on a promise built
// by a `Promise` static: §27.2.2.1 calls it with `this` undefined and
// one argument, a throw rejects the derived promise, a returned
// thenable is adopted.
function add1(v: number): number { return v + 1 }
function seven(): number { return 7 }
function inc(v: number): number { return v + 1 }
function viaThenable(v: number): Promise<number> { return Promise.resolve(v * 10) }
function boom(v: number): number { throw new Error("boom " + v) }
function argCount(v: number): number { return arguments.length * 100 + v }
function recover(e: any): number { return e.message.length }
function both(a: number, b?: number): number { return b === undefined ? a + 1000 : -1 }
function who(v: number): string { return typeof this + " " + v }
function onFinally(v?: number): void { console.log("finally", v === undefined) }
function onReject(e: any): number { return -1 }

console.log(await Promise.resolve(1).then(add1))
console.log(await Promise.resolve(2).then(seven))
console.log(await Promise.resolve(3).then(viaThenable))
console.log(await Promise.resolve(4).then(boom).catch(recover))
console.log(await Promise.resolve(5).then(argCount))
console.log(await Promise.reject(new Error("abc")).catch(recover))
console.log(await Promise.resolve(6).then(both))
console.log(await Promise.resolve(7).then(who))
console.log(await Promise.resolve(8).then(inc).then(inc))
console.log(await Promise.resolve(9).finally(onFinally))
console.log(await Promise.reject(new Error("x")).then(add1, onReject))
console.log(await Promise.resolve(10).then(add1, onReject))

// the function value keeps its identity and its own properties
const f = add1
console.log(f === add1, add1.length, add1.name)
;(add1 as any).tag = "t"
console.log(await Promise.resolve(11).then(add1), (add1 as any).tag)

// a handler that captures stays a closure
function makeAdder(k: number): (v: number) => number {
  function addK(v: number): number { return v + k }
  return addK
}
console.log(await Promise.resolve(12).then(makeAdder(100)))
let base = 1000
const addBase = (v: number): number => v + base
base = 2000
console.log(await Promise.resolve(13).then(addBase))

// a rebound binding hands on whatever it holds when the call runs
let h: (v: number) => number = add1
h = seven
console.log(await Promise.resolve(14).then(h))

let total = 0
for (let i = 0; i < 1000; i = i + 1) {
  total = total + (await Promise.resolve(i).then(inc))
}
console.log(total)
