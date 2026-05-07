// T-15.g.6.c (v0.5.0) — `console.log(await heap_promise)` direct
// form now prints the value (was previously printing the str / arr
// ptr because Type::Promise was type-erased at SSA — the await
// Member-access dispatch always returned Type::I64).
//
// The check::Type per-Expr map (from check::check_with_types,
// wired through LowerCtx in T-15.g.6.b) now drives result-type
// inference at the await Member-access site. For heap inner T,
// the runtime helper's int64_t result is cast to ptr-shape via
// the new `InstKind::IntToPtr` so downstream Member / Index
// instructions dispatch correctly.

console.log(await Promise.resolve('hello'))                // hello
console.log(await Promise.resolve(42))                     // 42
console.log((await Promise.resolve('foo')) + ' bar')       // foo bar

let xs = [10, 20, 30]
let arr_p = Promise.resolve(xs)
console.log((await arr_p).length)                          // 3
console.log((await Promise.resolve([1, 2, 3]))[1])         // 2

async function getName(): string {
  return 'torajs'
}
console.log(await getName())                               // torajs
