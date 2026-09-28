// the entry's own namespace when the entry also re-exports: the
// re-exported fields come from the modules the clauses name, the
// entry's own fields stay its own bindings, and the entry evaluates once
import * as ns from "./main";
export let x = 1;
export function bump() { x++; }
const hidden = "h";
export { hidden as shown };
export { ox as y, obump } from "./o";
export * from "./p";
export default (console.log("default evaluated"), 7);
console.log("body");
bump();
ns.obump();
console.log(ns.x, x, ns.shown, ns.y, ns.px, Object.keys(ns).join(","), ns.bump === bump, ns.default);
