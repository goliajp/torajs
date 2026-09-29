// the entry's own namespace read through a named function called
// ahead of the export's declaration: the index and member reads fold
// to the binding, whose dead zone the hoisted function still observes
import * as ns from "./main.ts";
function namedGet() {
  return ns["x"];
}
function namedDot() {
  return ns.x;
}
try {
  console.log(namedGet());
} catch (e) {
  console.log("get", e instanceof ReferenceError, (e as Error).message);
}
try {
  console.log(namedDot());
} catch (e) {
  console.log("dot", e instanceof ReferenceError);
}
export let x = 1;
console.log(namedGet(), namedDot());
