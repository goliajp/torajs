// Dynamic-function strict early errors (§20.2.1.1 steps 17/22 →
// §15.2.1): a body whose directive prologue opens with 'use strict'
// throws SyntaxError AT CREATION for duplicate params, reserved-word
// params, assignment to eval/arguments, and `with`. The same shapes
// stay legal in a sloppy body.
let n = 0;
try {
  new Function("param_1", "param_2", "param_1", '"use strict"; return 0;');
} catch (e: any) {
  if (e instanceof SyntaxError) n += 1;
}
try {
  new Function('"use strict"; with ({}) {}');
} catch (e: any) {
  if (e instanceof SyntaxError) n += 10;
}
try {
  const f: any = new Function(" ", '"use strict"; eval = 42; ');
  f();
} catch (e: any) {
  if (e instanceof SyntaxError) n += 100;
}
try {
  new Function("eval", '"use strict"; return eval;');
} catch (e: any) {
  if (e instanceof SyntaxError) n += 1000;
}
const ok: any = new Function("a", "a", "return a + 1;");
console.log(n, ok(1, 2));
