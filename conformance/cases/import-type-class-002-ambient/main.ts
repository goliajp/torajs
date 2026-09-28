// a class imported as a type only, with no value import anywhere in
// the program landing it: the type still names the class's instance
// shape (own fields, own and inherited methods), and m never evaluates
import type { C } from "./m";
import type { C as K } from "./m";
function describe(c: C): string { return c.name() + ":" + c.tag() + ":" + c.v; }
let held: K | undefined;
const pool: C[] = [];
console.log(typeof describe, describe.length, held === undefined, pool.length);
