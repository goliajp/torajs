// Object.fromEntries takes any iterable of entries, and its abrupt
// completions follow AddEntriesFromIterable
function* gen() { yield ["a", 1]; yield ["b", 2] }
console.log(JSON.stringify(Object.fromEntries(gen())))
const custom: any = { [Symbol.iterator]() { let i = 0; return { next() { i++; return i <= 2 ? { done: false, value: ["k" + i, i * 10] } : { done: true } } } } }
console.log(JSON.stringify(Object.fromEntries(custom)))
console.log(JSON.stringify(Object.fromEntries(new Map([["m", 3]]).entries())))
console.log(JSON.stringify(Object.fromEntries([["x", 1]].values().map((e: any) => [e[0] + "!", e[1]]))))
class Boom extends Error {}
let closed = 0
const throwsNext: any = { [Symbol.iterator]() { return { next() { throw new Boom("next") }, return() { closed++; return {} } } } }
try { Object.fromEntries(throwsNext) } catch (e: any) { console.log(e instanceof Boom, e.message, closed) }
const badEntry: any = { [Symbol.iterator]() { return { next() { return { done: false, value: 5 } }, return() { closed++; return {} } } } }
try { Object.fromEntries(badEntry) } catch (e: any) { console.log(e.constructor.name, closed) }
let reads = 0
const keyThrows: any = { [Symbol.iterator]() { let n = 0; return { next() { n++; return { done: false, value: { get "0"() { throw new Boom("key") }, get "1"() { reads++; return 1 } } } }, return() { closed++; return {} } } } }
try { Object.fromEntries(keyThrows) } catch (e: any) { console.log(e instanceof Boom, e.message, closed, reads) }
try { Object.fromEntries(5 as any) } catch (e: any) { console.log(e.constructor.name) }
