// §16.2.2.5 ModuleRequests — an empty ImportClause / ExportsList
// still names a requested module: both files load and evaluate, in
// clause order, before the entry's own body.
import {} from "./a.ts";
export {} from "./b.ts";
console.log("main");
