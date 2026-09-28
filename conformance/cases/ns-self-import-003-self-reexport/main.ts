// the entry re-exporting its own bindings back through itself
// (`export { local as face } from "./main"`, `export * from "./main"`):
// the new face names the same binding, a star at itself adds nothing,
// and the namespace stays the entry's own (live, evaluated once)
import * as ns from "./main";
export let local1 = 1;
const h = "h";
export { h as shown };
export { local1 as indirect, shown as twice } from "./main";
export * from "./main";
export function bump() { local1++; }
console.log("body");
bump();
console.log(ns.local1, ns.indirect, ns.twice, Object.keys(ns).join(","), ns.bump === bump);
