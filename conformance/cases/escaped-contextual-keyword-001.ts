// §5.1.5 — `of`, `static`, `target`, `as` and `from` are names except
// in the one production that spells them, and there they are terminal
// symbols that must appear exactly as written. Spelled with a unicode
// escape they are the identifier again, which the production cannot
// take. The import / export shapes are module-only and so cannot be
// probed through eval; checked by hand against the module goal:
//   import {a \u0061s b} from "m"     import * \u0061s ns from "m"
//   import {} \u0066rom "m"           export {} \u0066rom "m"
//   export default \u0061sync function f() {}
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("function f() { return new.t\\u0061rget; }")));
console.log(caught(() => eval("class C { st\\u0061tic m() {} }")));
console.log(caught(() => eval("class C { st\\u0061tic {} }")));
console.log(caught(() => eval("for (var x o\\u0066 []) ;")));
console.log(caught(() => eval("async function g() { for await (var x o\\u0066 []) ; }")));

// As plain names the escaped spellings are ordinary identifiers.
var \u0061s = 1;
var \u0066rom = 2;
var o\u0066 = 3;
console.log(as + from + of);
const t = { t\u0061rget: 4, st\u0061tic: 5 };
console.log(t.target, t.static);
function nt() {
  return new.target === undefined;
}
console.log(nt());
for (var x of [6]) console.log(x);
