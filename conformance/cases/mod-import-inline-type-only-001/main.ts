// verbatimModuleSyntax keeps `import { type P } from "m"` as
// `import {} from "m"`: nothing is bound, but m is still requested
// and evaluates. bun without a tsconfig elides the whole declaration
// instead (TypeScript's pre-verbatim default), so `main.expected` pins
// the verbatim answer.
import { type P } from "./lib.ts";
const p: P = { x: 1 };
console.log("main", p.x);
