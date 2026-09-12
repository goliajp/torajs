// A sloppy direct eval whose source is nothing but declarations used
// to collapse to `undefined` when no identifier in the program spelled
// the declared name — but a name can be read from text this compiler
// parses later: a Script (`$262.evalScript`), another eval, or a
// `Function(...)` body. Those reads are as real as one written in the
// program body, so the declaration must land on the global (§19.2.1.1
// sloppy direct eval shares the caller's VariableEnvironment; at the
// top level that is the global). Before: `ReferenceError: keep is not
// defined` from the Script, while `console.log(keep)` in the program
// itself worked.
eval('var keep = 5;');
__t262_evalScript('console.log("script-reads", keep);');
eval('function later() { return "fn"; }');
__t262_evalScript('console.log("script-calls", later());');
eval('var viaEval = 6;');
console.log("eval-reads", eval('viaEval'));
eval('var viaFn = 7;');
console.log("function-reads", Function('return viaFn;')());
