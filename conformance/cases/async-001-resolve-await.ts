// T-15 (v0.5.0) — basic Promise.resolve + await without user-class.
// Locks in the v0.5 milestone: async/await works without the user
// declaring `class Promise<T>` themselves.

let p = Promise.resolve(42)
console.log(await p)
console.log(await Promise.resolve(7) + 1)
