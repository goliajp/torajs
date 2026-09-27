// §16.2.1.5 — importing a module by name evaluates its body once,
// before the importer's own statements. The resolver used to inject
// only the requested declarations, dropping every other top-level
// statement of the lib: `get()` answered 10 and `n` 1.
import { get, bump, n } from "./lib.ts";
console.log("main " + get());
bump();
console.log("main n " + n);
