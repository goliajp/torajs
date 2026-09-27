// An aliased type import names the imported type under the local
// spelling: `import type { A as B }`, `import { type A as B }`, a
// generic alias, and an alias used only inside another module. Each
// local name used to be an unknown type.
import type { Pt as P, Label as L, Box as B } from "./lib.ts";
import { type Pt as Q, k } from "./lib.ts";
import { norm1 } from "./mid.ts";

const p: P = { x: 1, y: 2 };
const q: Q = { x: k, y: -3 };
const name: L = "origin";
const b: B<number> = { v: 7 };
const bs: B<string> = { v: "s" };
console.log(p.x + p.y, q.x + q.y, name, b.v, bs.v);
console.log(norm1(q));
