// S127-3 — Object.assign(target, ...sources) per §20.1.2.1 N-source.
// Pre-fix only single-source `Object.assign(target, source)` typechecked;
// `Object.assign({}, a, b)` rejected with "expects 2 args (single-source
// MVP)". Subset stays: target + every source must share the same struct
// type (no shape-merge / superset semantics yet).
type S = { x: number; y: number; z: number }
const a: S = { x: 1, y: 2, z: 3 }
const b: S = { x: 0, y: 99, z: 0 }
const c: S = { x: 0, y: 0, z: 7 }
const merged: S = { x: 0, y: 0, z: 0 }
Object.assign(merged, a, b, c)
// last-source-wins per spec: x from b (overwrites a's 1), y from b (99),
// z from c (overwrites a's 3 and b's 0 → 7).
console.log("merged.x =", merged.x)
console.log("merged.y =", merged.y)
console.log("merged.z =", merged.z)
// chain return value still typechecks
const r: S = Object.assign({ x: 0, y: 0, z: 0 } as S, a, b)
console.log("chain.x =", r.x, "chain.y =", r.y, "chain.z =", r.z)
// 3+ sources extended to 5 — exhaustive merge
const s1: S = { x: 1, y: 0, z: 0 }
const s2: S = { x: 0, y: 2, z: 0 }
const s3: S = { x: 0, y: 0, z: 3 }
const s4: S = { x: 100, y: 0, z: 0 }
const big: S = { x: 0, y: 0, z: 0 }
Object.assign(big, s1, s2, s3, s4)
console.log("big.x =", big.x, "big.y =", big.y, "big.z =", big.z)
