// Script code, not module code: at a module's top level a `function`
// is lexically declared, so it may not share a name with a `var`
// (§16.2.1.1). A dynamic import resolves its target into a Module
// Record, so this file is a SyntaxError there.
var smoosh; function smoosh() {}
