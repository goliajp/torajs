// An async function VALUE spelled `function` (or a nested async
// declaration, which becomes one) inside an async function that is
// itself a machine, capturing a local of that machine: the nested
// machine's frame link used to ride the value's own `this` — the
// receiver of whoever calls it, an `any` — and the any-widened clone
// of the nested factory stored the box into the iterator-typed
// field; the first `next()` then looped in the method dispatch
// (stack overflow, exit 139). The frame is now captured lexically
// outside the value. Declaration and expression forms, with and
// without a try region, and a value called through a detached
// reference (its `this` is not the machine). (An object-literal async
// METHOD capturing a machine local is a separate, older hole — the
// capture is an unknown identifier there — and is registered.)
async function decl(): Promise<string> {
  var seen = "";
  async function f() { seen += "t"; await null; seen += "F"; }
  await f();
  return seen;
}
async function expr(): Promise<string> {
  var seen = "";
  const f = async function () { try { seen += "t"; await null; } finally { seen += "f"; await null; seen += "F"; } };
  await f();
  return seen;
}
async function detached(): Promise<string> {
  var seen = "";
  async function f() { seen += "t"; await null; seen += "F"; }
  const g = f;
  await g();
  return seen;
}
async function main(): Promise<void> {
  console.log("decl", await decl());
  console.log("expr", await expr());
  console.log("detached", await detached());
}
main();
