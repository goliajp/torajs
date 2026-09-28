// a class exported under several bare-clause faces (`export { C as D,
// C as E }`): a type-only import of any face resolves to the class
// declaration, and m never evaluates
import type { E } from "./m";
import type { D } from "./m";
function f(e: E): string { return e.tag(); }
function g(d: D): string { return d.tag(); }
let held: E | undefined;
console.log(typeof f, f.length, typeof g, held === undefined);
