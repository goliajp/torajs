// A `type` modifier on one specifier erases that specifier only; the
// value specifier keeps the module requested and evaluated once.
import { type P, f } from "./lib.ts";
const p: P = f();
console.log(p.x);
