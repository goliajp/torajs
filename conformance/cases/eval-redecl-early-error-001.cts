// A redeclaration inside eval code is an early error of that code, and
// §19.2.1.1 step 12 raises it as a SyntaxError when the eval runs; the
// rest of the program compiles and runs. Each probe below is caught,
// and the legal shapes beside them still evaluate.

function probe(tag: string, run: () => void) {
  try {
    run();
    console.log(tag, "ran");
  } catch (e) {
    console.log(tag, e instanceof SyntaxError);
  }
}

// §14.2.1 — a Block's lexical names against each other and its vars.
probe("let-fn", () => eval(`{ let x = 42; function x() {} }`));
probe("const-fn", () => eval(`{ const x = 42; function x() {} }`));
probe("fn-let", () => eval(`{ function x() {} let x = 1; }`));
probe("let-let", () => eval(`{ let x; let x; }`));
probe("let-var", () => eval(`{ let x; { var x; } }`));
// §B.3.3.4 — two plain functions in one sloppy block are allowed...
probe("fn-fn-sloppy", () => eval(`{ function x() {} function x() {} }`));
// ...but not in strict code.
probe("fn-fn-strict", () => eval(`"use strict"; { function x() {} function x() {} }`));
// §14.12.1 — every clause of a CaseBlock shares one scope.
probe("case", () => eval(`switch (1) { case 0: let y; default: let y; }`));
// §16.1.1 — the eval body's own top level: functions are var names.
probe("top-let-var", () => eval(`let z; var z;`));
probe("top-let-fn", () => eval(`let z; function z() {}`));
probe("top-fn-fn", () => eval(`function z3() {} function z3() {}`));
// §15.2.1 — a function body the text declares.
probe("fn-body", () => eval(`function g() { let w; var w; }`));
// §15.7.1 — a class method body is strict, and a body of its own.
probe("method", () => eval(`class C { m() { let w; { var w; } } }`));

// A legal text still runs and its effects are seen.
let seen = 0;
eval(`{ function h() { return 5; } function h() { return 6; } seen = h(); }`);
console.log("seen", seen);

// §15.2.1 — a destructuring parameter's names are parameters: a `var`
// or a function in the body may share them.
console.log(eval("function pd({ a }) { var a; return a; } pd({ a: 1 })"));
console.log(eval("function pd2([a]) { function a() {} return typeof a; } pd2([1])"));
console.log(Function("{ a }", "var a; return a")({ a: 4 }));
probe("param-let", () => eval(`function pd3({ a }) { let a; }`));
