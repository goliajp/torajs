// §16.1.7 step 5.c — a binding a sloppy direct eval introduced is
// configurable, so it is not a restricted global property: a later
// Script's `let` / `const` of the same name is legal and shadows it
// (test262 `script-decl-lex-var-declared-via-eval`). Expected output is
// node's line for line with `$262.evalScript` = `vm.runInThisContext`.
// Before: the eval's declaration and the Script's, inlined flat, were
// one name declared twice to the checker (compile-time reject).
eval('var test262Var;');
eval('function test262Fn() {}');
__t262_evalScript('let test262Var = 1;');
console.log("var-shadowed", test262Var);
__t262_evalScript('const test262Fn = 2;');
console.log("fn-shadowed", test262Fn, typeof test262Fn);
eval('var keep = 5;');
console.log("kept", keep);
