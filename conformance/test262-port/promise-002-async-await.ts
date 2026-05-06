// T-15.h (v0.5.0) — `async function` + `await` migrated from the
// L.2 user-class MVP to the built-in Promise<T>. desugar_async now
// rewrites each `return e` inside an async body to
// `return Promise.resolve(e)` (no shared `__async_p` state); the
// built-in Promise carries the resolved i64 value in its heap
// header's value slot, and `await p` (= `p.value`) drains the
// microtask queue then reads the value.
//
// Pending Promise + executor-style `new Promise(executor)` is
// deferred to T-15.g.5 alongside the `(resolve, reject)` arrow-arg
// shape. The pre-T-15.h `pending_aware` case (which relied on
// `new Promise(0)` building a stuck-pending promise) is dropped
// here since that path now resolves through built-in Promise.

function presolved(v: number): Promise<number> {
  return Promise.resolve(v)
}

// Trivial — no awaits, no compute
async function trivial(): number {
  return 42
}

// Single await
async function inc(): number {
  let x = await presolved(10)
  return x + 1
}

// Multiple awaits in sequence
async function combine(): number {
  let a = await presolved(100)
  let b = await presolved(200)
  let c = await presolved(33)
  return a + b + c
}

// await mixed with non-await arithmetic
async function mixed(seed: number): number {
  let x = await presolved(seed)
  let y = x * 2 + 1
  let z = await presolved(y)
  return z + 100
}

function check(): number {
  if (trivial().value !== 42) { throw '#1 trivial' }
  if (inc().value !== 11) { throw '#2 inc' }
  if (combine().value !== 333) { throw '#3 combine' }
  if (mixed(7).value !== 115) { throw '#4 mixed' }   // 7→x; y=15; z=15; +100=115

  return 0
}
console.log(check())
