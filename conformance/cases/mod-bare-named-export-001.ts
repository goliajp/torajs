// P13-S4b — bare named exports (`export { a, b as c }`, no `from`)
// resolve through both import shapes: the named import binds the
// EXPORTED names, and a side-effect import runs the lib's top-level
// statements while dropping the consumer-less export face.
import { time, f } from "./mod-bare-named-export-001-lib.ts";
console.log(time, f());
