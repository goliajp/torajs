// T-15.h (v0.5.0) — `async function` desugar to built-in
// Promise.resolve. No user-class Promise<T> needed.

async function trivial(): number {
  return 42
}

async function combine(): number {
  let a = await trivial()
  let b = await Promise.resolve(100)
  return a + b
}

console.log(await trivial())          // 42
console.log(await combine())          // 142
console.log(await trivial() + await trivial()) // 84
