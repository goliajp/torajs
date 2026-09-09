// §16.2.2 ImportedBinding is a BindingIdentifier and §16.2.3
// ModuleExportName is an IdentifierName. Five slots in the two clause
// parsers matched `Token::Ident` instead: the default-import binding,
// the `* as ns` binding, the import specifier's source name and its
// alias, and the export clause's names. So `import async from …`,
// `import * as async`, `import { a as type }`, `import { default as
// async }` and `export { x as let }` were all parse errors that node
// accepts, while `import { a as let }` stays refused — a module
// reserves that word, and the goal gate is what says so.
import async from "./import-export-keyword-names-001-dep.ts";
import * as type from "./import-export-keyword-names-001-dep.ts";
import { a as asyncAlias, b as typeAlias } from "./import-export-keyword-names-001-dep.ts";
console.log(async, type.a, asyncAlias, typeAlias);
var local = 4;
export { local as let, local as async, local as default };
