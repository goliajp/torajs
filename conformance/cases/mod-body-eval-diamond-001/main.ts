// §16.2.1.5 — a module two importers share evaluates once, before
// either of them; siblings run in request order.
import { B } from "./b.ts";
import { C } from "./c.ts";
console.log("main " + (B + C));
