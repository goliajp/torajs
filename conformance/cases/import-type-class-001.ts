// a class imported as a type only, under its own name or an alias,
// names the class its value walk landed, wherever that landing put it
import type { C as K } from "./import-type-class-001-lib";
import { type C, mk } from "./import-type-class-001-lib";
import type { C as C2 } from "./import-type-class-001-lib";
function take(c: C): number { return c.v; }
const k: K = mk();
const c2: C2 = k;
console.log(take(mk()), k.v, c2.v, k instanceof Object);
