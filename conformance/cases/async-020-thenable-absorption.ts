// T-19.f (v0.5.0) — thenable absorption per ES2015. `Promise.resolve
// (p)` when `p` is itself a Promise must return a Promise that
// resolves with p's resolved value, NOT a Promise wrapping p as
// its value. The type system collapses Promise<Promise<T>> to
// Promise<T>; the runtime helper __torajs_promise_resolve_thenable
// reads inner state + value, inc's the inner's resolved-value rc
// when value_is_heap so outer's drop and inner's drop don't race.
//
// Sync MVP: inner is always FULFILLED or REJECTED at the moment we
// observe it (no real suspension yet). Pending inner → rejected
// outer with placeholder reason; full callback fan-in lands with
// T-16 state-machine async/await.

let inner_num = Promise.resolve(42)
let outer_num = Promise.resolve(inner_num)
console.log(await outer_num)              // 42 — number value, not [object Promise]

let inner_str = Promise.resolve('hello')
let outer_str = Promise.resolve(inner_str)
console.log(await outer_str)              // hello

let inner_arr = Promise.resolve([1, 2, 3])
let outer_arr = Promise.resolve(inner_arr)
let r: number[] = await outer_arr
console.log(r[0], r[1], r[2])             // 1 2 3

// Two outers absorbing the same inner — both inc inner.value's rc
// independently, both drop independently; box frees once exactly.
let outer_str_2 = Promise.resolve(inner_str)
console.log(await outer_str_2)            // hello
