// a library that imports its own namespace, re-exports through itself
// and through a module that re-exports back into it, requested by an
// entry that names only one export: the namespace is the library's own
// bindings (no second copy), keys sorted, both modules evaluated once
import { keys } from "./lib";
console.log(keys());
