// a library's write to one of its own import bindings is the same
// runtime TypeError the entry's is (§16.2.1.5 CreateImportBinding —
// an import binding is immutable in the importer); a library's write
// to its own exported binding stays the legal live-binding write
import { tryWrite, tryIncr, bump, counter } from "./lib.ts";
console.log(tryWrite(), tryIncr());
bump();
console.log(counter);
