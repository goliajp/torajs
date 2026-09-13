// A generator-lifted local whose initializer the desugar's field
// sniff could not read was pinned `number` — a guess, not a reading —
// and every wrong guess became a field the checker refused: `const fn
// = function (a, b) {}` ("field is Number, value is Function"), a
// thenable literal `{ then(resolve, reject) {…} }` ("…value is
// Struct"), `var wasDisposed = stack.disposed` (a Boolean read off a
// class instance, pinned Number and tripping `__t262_sameValue`'s
// generic). Blade 2 moving `asyncTest(async function () {…})` bodies
// into generators put test262's Array.fromAsync and
// AsyncDisposableStack cases on it. Such a local is an `any` slot now;
// only the parser's own for-of counter temp keeps `number`.
class Stack {
  disposed = false;
  dispose() {
    this.disposed = true;
    return Promise.resolve(1);
  }
}
async function decl() {
  const fn = function (a, b) {};
  let n = 0;
  const t = { then(resolve, reject) { n++; resolve(7); } };
  var stack = new Stack();
  var before = stack.disposed;
  var p = stack.dispose();
  var during = stack.disposed;
  await p;
  console.log("decl", typeof fn, fn.length, typeof t.then, before, during, stack.disposed);
}
function runner(f: any) {
  return f();
}
runner(async function () {
  const fn = function (a, b) {};
  const t = { then(resolve, reject) { resolve(9); } };
  var stack = new Stack();
  var before = stack.disposed;
  await stack.dispose();
  console.log("value", typeof fn, fn.length, typeof t.then, before, stack.disposed);
});
function* gen() {
  const fn = function (a, b, c) {};
  const t = { then(resolve, reject) { resolve(1); } };
  yield fn.length;
  yield typeof t.then;
}
const it = gen();
console.log("gen", it.next().value, it.next().value);
decl();
