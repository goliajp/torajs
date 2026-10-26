// `async function` + `await` on the built-in Promise<T>: an async
// function's result is a Promise that settles after its body runs,
// so the checks await each result (reading a field of the pending
// Promise synchronously is not JS: a Promise has no `value` property).

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

async function check(): Promise<number> {
  if ((await trivial()) !== 42) { throw '#1 trivial' }
  if ((await inc()) !== 11) { throw '#2 inc' }
  if ((await combine()) !== 333) { throw '#3 combine' }
  if ((await mixed(7)) !== 115) { throw '#4 mixed' }   // 7→x; y=15; z=15; +100=115

  return 0
}
check().then((v) => console.log(v))
