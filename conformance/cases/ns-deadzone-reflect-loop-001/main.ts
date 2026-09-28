// Reflective reads of a module namespace field whose binding is still
// in its dead zone raise a ReferenceError; every face must raise on
// every iteration and leave nothing behind.
import * as ns from "./main.ts";

const N = 2000;
const counts: number[] = [];
const faces: Array<() => unknown> = [
  () => Object.keys(ns),
  () => Object.values(ns),
  () => Object.entries(ns),
  () => Object.getOwnPropertyDescriptor(ns, "x"),
  () => Object.getOwnPropertyDescriptors(ns),
  () => Object.assign({}, ns),
  () => { for (const k in ns) { return k; } return ""; },
  () => ns.x,
];
for (const face of faces) {
  let caught = 0;
  let quiet = 0;
  for (let i = 0; i < N; i++) {
    try { face(); quiet++; } catch (e) { if (e instanceof ReferenceError) caught++; }
  }
  counts.push(caught * 10 + quiet);
}
console.log(counts.join(","));
export let x = 1;
console.log(Object.keys(ns).join(","), Object.values(ns).join(","), (ns as any).x);
