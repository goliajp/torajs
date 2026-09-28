// a class imported as a type only under the spelling a bare export
// clause gave it (`export { C as D }`): the type resolves to the class
// declaration, and m never evaluates
import type { D } from "./m";
function f(d: D): string { return d.tag(); }
let held: D | undefined;
console.log(typeof f, f.length, held === undefined);
