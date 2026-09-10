// §13.15.1 — sloppy code may assign to `arguments` itself, and the
// assignment rebinds the local: `arguments = 42`, `++arguments`, a
// destructuring target. The materialized `__torajs_arguments` was a
// const `any[]` (or, off the faces, no local at all — a
// ReferenceError), so each of these was refused (nine test262 cases:
// for-await-of dstr no-strict, sm/strict/11.4.x / 11.13.x,
// sm/expressions/destructuring-pattern-parenthesized).
// `.expected` because bun's TypeScript front end rejects the
// assignment as an invalid target even in a sloppy .cts; node on the
// same source as .js is the semantic oracle behind the file.
function f(a) { arguments = 42; return [arguments, a]; }
function h(a) { var r = arguments.length; arguments = [9]; return [r, arguments[0]]; }
function k(a) { [arguments = 4] = []; return arguments; }
function m(a) { const before = arguments[1]; arguments = 7; return [before, arguments, typeof arguments === "number"]; }
function n(a) { arguments = a + 1; return arguments; }
console.log(f(1, 2), h(1, 2, 3), k(1), m(1, 2), n(41));
let iterCount = 0;
async function fn() {
  for await ([arguments = 4, eval = 5] of [[]]) {
    console.log(arguments, eval);
    iterCount += 1;
  }
}
var eval;
fn().then(() => console.log(iterCount));
