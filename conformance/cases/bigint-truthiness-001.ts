// ES §7.1.4 ToBoolean(BigInt) — 0n is falsy, every other BigInt is
// truthy. tr previously routed BigInt through coerce_to_bool's
// heap-pointer fallback (non-null → true), so every BigInt was
// truthy including 0n, breaking `if (0n)`, `Boolean(0n)`, `!0n`,
// `0n && x`, `0n || x`, and every ??=/||=/&&= that hinges on
// BigInt truthiness.

let a = 0n;
console.log("Boolean(0n):", Boolean(a));
console.log("Boolean(1n):", Boolean(1n));
console.log("!0n:", !a);
console.log("!1n:", !1n);

if (0n) { console.log("if(0n): truthy"); } else { console.log("if(0n): falsy"); }
if (1n) { console.log("if(1n): truthy"); } else { console.log("if(1n): falsy"); }

// Short-circuit under BigInt operand (fixed by both the ES2021
// short-circuit desugar and this truthiness fix — the assign here
// exercises the falsy-lhs branch for &&, truthy-lhs for ||).
let g = 0n;
console.log("(g &&= 1n) === 0n:", (g &&= 1n) === 0n);
let h = 0n;
console.log("(h ||= 1n) === 1n:", (h ||= 1n) === 1n);

// Any lane (BigInt through anyv_to_bool) — logical operand promoted
// through Any due to mixed-arm ternary.
const box: any = 0n;
console.log("!!(any 0n):", !!box);
const box2: any = 3n;
console.log("!!(any 3n):", !!box2);
