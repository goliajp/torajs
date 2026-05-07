// T-19.o (v0.5.0) — heterogeneous `Promise<T>.then(cb)` /
// `.catch(cb)` where cb's return type U differs from T per
// ES2015. check.rs probes cb's actual signature: if its param
// matches T and its return is a primitive the runtime helper
// can pack through i64 (Number / String / Boolean), the result
// is `Promise<U>`. Same-T case (T == U) still falls through
// to the method-table arm so the common `(T) => T` shape stays
// fast-path.
//
// Heap T → U (Array, Struct, Date, RegExp) and closure-cb
// variants of T → U land alongside generic-method TypeVar
// substitution (T-15.g.4); the runtime side already i64-packs
// any 64-bit-shaped value cleanly.

function strLen(s: string): number { return s.length }
function tagN(v: number): string { return 'n=' + v }
function isPos(reason: number): boolean { return reason > 0 }
function fromBool(b: boolean): number { return b ? 1 : 0 }

let p1 = Promise.resolve('hello').then(strLen)
console.log(await p1)                         // 5

let p2 = Promise.resolve(42).then(tagN)
console.log(await p2)                         // n=42

let p3 = Promise.reject(7).catch(isPos)
console.log(await p3)                         // true

let p4 = Promise.resolve(true).then(fromBool)
console.log(await p4)                         // 1

// Chain through different Ts: Number → String → Number.
let p5 = Promise.resolve(99).then(tagN).then(strLen)
console.log(await p5)                         // 4 ('n=99'.length)
