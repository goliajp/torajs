// §8.6.2 + §15.8.4 — a parameter default that reads its own parameter
// or a later one hits the TDZ; in an async function that throw is the
// REJECTION of the returned promise, the body never runs, and the
// call itself does not throw. Every async form: declaration, arrow,
// function expression, class method (instance / static), object
// method. A supplied argument never evaluates the default.
var x = 0;
var callCount = 0;
const report = (tag: string, p: Promise<any>) =>
  p.then(
    (_) => console.log(tag, "resolved (wrong)"),
    (e) => console.log(tag, e.constructor === ReferenceError, callCount),
  );
async function decl(x = x) { callCount = callCount + 1; }
const arrow = async (x = y, y) => { callCount = callCount + 1; };
const fnexpr = async function named(x = x) { callCount = callCount + 1; };
class C {
  async m(x = x) { callCount = callCount + 1; }
  static async s(x = y, y) { callCount = callCount + 1; }
}
const obj = {
  async om(x = y, y) { callCount = callCount + 1; },
};
async function main() {
  await report("decl", decl());
  await report("arrow", arrow());
  await report("fnexpr", fnexpr());
  await report("method", C.prototype.m());
  await report("static", C.s());
  await report("objmeth", obj.om());
  // a supplied argument: the default is never evaluated, the body runs
  await decl(5);
  await arrow(1, 2);
  console.log("supplied", callCount);
}
main();
