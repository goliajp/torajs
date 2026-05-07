// T-19.m (v0.5.0) — user-declared `function main()` no longer
// collides with the synthesized OS-entry `main` symbol. Pre-fix
// the LLVM module had two functions named `main` (i32-returning
// synthesized entry + i64-returning user fn) → verify error
// `Function return type does not match operand type of return
// inst`. The new `rename_user_main` AST pass renames the user's
// `main` to `__user_main` and rewrites every reference. Idents
// in nested expression positions (struct fields, member access)
// are intentionally left alone.

async function delay(ms: number): number {
  return ms
}

async function main(): number {
  let a = await delay(10)
  let b = await delay(20)
  return a + b
}

console.log(await main())                  // 30

// Synchronous user-declared `function main()` covers the same
// rename path without async desugar in the picture.
function compute(n: number): number {
  return n * n
}

function syncMain(): number {
  return compute(7)
}
console.log(syncMain())                    // 49
