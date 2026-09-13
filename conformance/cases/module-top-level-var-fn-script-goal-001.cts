// §16.1.1 — a Script's top level var-scopes a plain `function`, so a
// `var` of the same name is legal (last declaration wins). The same
// two lines are an early SyntaxError under the module goal
// (§16.2.1.1; test262 `module-code/early-*`, dynamic-import
// `eval-script-code-target`).
var smoosh; function smoosh() { return 1; }
console.log("ran", typeof smoosh, smoosh());
