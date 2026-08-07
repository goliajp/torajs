// Direct eval of a string literal, inlined at the call site.
// §19.2.1.1 PerformEval, strict branch: eval gets its own
// VariableEnvironment as well as its own LexicalEnvironment, so nothing
// it declares escapes — but everything it declares is visible to the
// rest of the eval'd program itself.

eval("console.log('top-level eval ran');");

// a lexical declaration used within the same eval
eval("let y = 1; console.log(y + 1);");

// eval inside a function body
function f() {
  eval("let x = 5; console.log(x * 2);");
}
f();

// `var` is visible inside the eval; strict mode keeps it from leaking
eval("var v = 9; console.log(v);");

// a function declared and called entirely within one eval
eval("function g() { return 'from g'; } console.log(g());");

// eval in a loop body runs once per iteration
for (let i = 0; i < 2; i++) {
  eval("console.log('loop body');");
}

// several statements, including control flow
eval("let n = 0; for (let i = 0; i < 3; i++) { n += i; } console.log(n);");

// nested eval — the inner literal is written inside the outer one
eval("eval(\"console.log('nested');\");");
