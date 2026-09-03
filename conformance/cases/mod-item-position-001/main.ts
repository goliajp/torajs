// §16.2 ModuleItem position (rotation 575). `import` / `export`
// DECLARATIONS are ModuleItems and a statement body cannot hold one —
// `if (false) export default null;` is a parse-phase SyntaxError.
// This fixture is the other half: every shape the position gate must
// NOT catch, because each is legal.
//
// Top-level declarations, all four import spellings and four export
// ones.
import * as ns from "./m.ts";
import { V as z } from "./m.ts";
import d, { W } from "./m.ts";
import "./m.ts";
export const local = 3;
export function ef(): number { return 4; }
export class EK { m(): number { return 5; } }
export { local as alias };
export * from "./m.ts";
console.log(ns.V, z, W, local, ef(), new EK().m());
// An ImportCall is an EXPRESSION (§13.3.10), legal wherever an
// expression is — including the statement bodies the gate refuses
// declarations in.
if (true) {
  import("./m.ts").then((m: any) => console.log("if-body", m.V));
}
function inFn(): any { return import("./m.ts"); }
inFn().then((m: any) => console.log("fn-body", m.W));
for (const x of [10]) {
  import("./m.ts").then((m: any) => console.log("for-body", m.V + x));
}
