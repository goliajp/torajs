// §16.2.1.5.3 — a named import evaluates the imported module's body
// once, before the importer: its top-level statements run in source
// order, and the non-exported counter they initialize is the one the
// exported functions read and write.
import { bump, read } from "./lib.ts";
console.log("main " + read());
console.log("bump " + bump());
console.log("bump " + bump());
console.log("read " + read());
