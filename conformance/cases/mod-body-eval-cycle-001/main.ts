// §16.2.1.5.3 — in a cycle the module reached second finishes first:
// main requests a, a requests b, b's request for a is already on the
// stack, so b's body runs before a's. Hoisted functions are visible
// across the cycle from the start.
import { fa } from "./a.ts";
import { fb } from "./b.ts";
console.log(fa(), fb());
