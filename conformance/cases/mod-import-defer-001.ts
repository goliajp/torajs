// import-defer proposal — `defer` is a CONTEXTUAL keyword only when
// the next token is `*`; as a plain default binding name it must
// keep working (test262 valid-default-binding-named-defer). The
// deferred-namespace form itself (`import defer * as ns`) parses +
// resolves eagerly in tr but is NOT in this fixture: bun 1.3.14
// rejects the syntax, so that face is covered by the test262 sweep
// instead of the bun oracle.
import defer from "./mod-import-defer-001-lib.ts";
console.log(defer);
