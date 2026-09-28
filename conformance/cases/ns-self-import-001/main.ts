// the entry's own namespace (`import * as ns from "./main"`) is made of
// the entry's own bindings: a mutable export read through it is live,
// and the entry evaluates once
import * as ns from "./main";
export let x = 1;
export function bump() { x++; }
const hidden = "h";
export { hidden as shown };
export default (console.log("default evaluated"), 7);
console.log("body");
bump();
console.log(ns.x, x, ns.shown, Object.keys(ns).join(","), typeof ns.bump, ns.bump === bump, ns.default);
