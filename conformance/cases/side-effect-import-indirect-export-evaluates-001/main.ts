// A side-effect import of b.ts: b's `export { x } from "./c.ts"`
// binds nothing anyone here can see, but c.ts is a requested module
// of b (§16.2.2.5) — it loads, its indirect export resolves
// (§16.2.1.6.4 step 9), and it evaluates before b and main.
import "./b.ts";
console.log("main");
