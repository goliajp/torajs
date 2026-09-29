// the entry importing its own named exports and default: each import
// binding is the entry's own binding (§16.2.1), not a copy that a
// second walk of the entry would inject — a later write through the
// export is read through the import spelling, and a lexical export
// read through its import spelling ahead of the declaration is the
// dead zone
import { x as y, f as g, l as l2, indirect } from "./main.ts";
import d from "./main.ts";
export var x = 99;
export function f() {
  return x;
}
export default 7;
console.log(y, g(), d);
try {
  console.log(l2);
} catch (e) {
  console.log(e instanceof ReferenceError);
}
x = 100;
console.log(y, g());
export let l = 1;
l = 2;
console.log(l2, indirect);
export { x as indirect } from "./main.ts";
