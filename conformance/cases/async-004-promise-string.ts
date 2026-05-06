// T-15.g.5 (v0.5.0) — Promise<string> via Call-arm dispatch.
// Runtime takes ownership of the heap str ref; promise_drop calls
// __torajs_value_drop_heap on its inner value.
//
// Known limitation: direct `console.log(await Promise.resolve("x"))`
// prints the raw ptr because Type::Promise is type-erased at SSA
// (no PromiseId interning yet — T-15.g.6 follow-up). Storing in an
// explicitly-typed `let s: string` first restores the type via the
// LetDecl arm's slot-shape coercion.

let p = Promise.resolve('hello')
let s: string = await p
console.log(s)
console.log(s + ' world')

let q = Promise.resolve('foo')
let t: string = await q
console.log(t)
