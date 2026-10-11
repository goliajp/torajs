// §16.2.1.5.3 — one module requested through the default, namespace
// and named forms is still one module record: its body runs once, and
// every form sees the same function and the same counter.
import hello from "./lib.ts";
import * as ns from "./lib.ts";
import { tag } from "./lib.ts";
console.log("main");
console.log(hello());
console.log(ns.default());
console.log(ns.tag, tag, ns.default === hello);
