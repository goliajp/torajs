// A module whose own statements read an export the importer binds
// only under an alias: the rename to the alias has to reach every
// reference inside the module, not just the declaration, or the
// module body reads a binding that was never declared.
import { f as g, k as j } from "./lib.ts";
import * as ns from "./lib.ts";
console.log(g() + j, typeof ns["☿"]);
