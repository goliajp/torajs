// a caller's local of the same name must not capture a name in the
// callee's parameter default: the default resolves in the callee's
// scope (ECMA-262 §10.2.11), not at the call site
const k = 1;
function f(y: number = k) {
  return y;
}
function f2(y: any = k + 1) {
  return y;
}
class C {
  m(y: number = k * 10) {
    return y;
  }
}
function callers() {
  const k = 2;
  return [f(), f2(), new C().m(), f(undefined), k].join(",");
}
console.log("top-level callee:", callers());

function mk(k: number) {
  const h = (y: number = k) => y;
  return h;
}
function direct() {
  const k = 888;
  const h2 = mk(6);
  return [h2(), k].join(",");
}
console.log("closure from elsewhere:", direct());
