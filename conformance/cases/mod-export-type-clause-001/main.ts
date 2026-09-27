// `export { type A, b }` exports the value only at runtime; a
// type-only re-export through another module does not evaluate it.
import { type Pair, g } from "./lib.ts";
import type { Pair as P2 } from "./mid.ts";
const a: Pair = g();
console.log(a[0] + a[1]);
