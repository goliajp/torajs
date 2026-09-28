// One export of a module, reached by several requests: a named import,
// a namespace import, a re-export through another module, and a
// function imported under two spellings in one clause. Each request
// re-reads the module, and its declaration used to land once per
// request — two functions, two objects (the initializer ran twice),
// two classes — so every identity check across the requests answered
// false.
import { a, b, c, G, G as H } from "./lib.ts";
import * as m from "./lib.ts";
import { d, e } from "./mid.ts";

console.log(a === m.a, b === m.b, c === m.c);
console.log(a === d, b === e, m.b === e);
console.log(G === H, G === m.G, H() + a());
b.k = 5;
console.log(m.b.k, e.k, new m.c() instanceof c);
