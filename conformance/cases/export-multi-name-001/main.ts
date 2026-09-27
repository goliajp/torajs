// One local binding exported under several names is one binding under
// each of them. Only the last name used to survive: the namespace
// object had `"s p"` but `M` and `N` were undefined, and a named
// import of an earlier name failed to resolve.
import * as ns from "./lib_ns.ts";
import { a, b, v, twice } from "./lib_named.ts";
import type { P2 } from "./lib_named.ts";

console.log(typeof ns.M, typeof ns.N, typeof ns["s p"]);
console.log(ns.M === ns.N, ns.N === ns["s p"], ns.M());
console.log(Object.keys(ns).join(","));
console.log(a === b, b === v, a.tag);
const p: P2 = { x: 3 };
console.log(twice(p.x));
