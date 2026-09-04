// §B.3.3 is written for a `FunctionDeclaration`. An
// `AsyncFunctionDeclaration` and an `AsyncGeneratorDeclaration` are
// not one, so the extra function-scoped binding is not theirs — even
// in sloppy code, where a plain declaration does get it.
//
// bun is not an oracle here: it reads a `.cts` as a module, and the
// question is what a SLOPPY script does. node follows the spec, so
// this checks against a `.expected`.

// The plain form keeps its Annex B binding.
{
  function plain() {}
}
console.log("plain", typeof plain);

// The three forms Annex B does not reach do not.
{
  async function asy() {}
}
console.log("async", typeof asy);

{
  async function* asyGen() {}
}
console.log("async-gen", typeof asyGen);

{
  function* gen() {}
}
console.log("gen", typeof gen);

// The same inside a function body, where the var binding would have
// been that body's local.
function body() {
  {
    function innerPlain() {}
  }
  {
    async function innerAsync() {}
  }
  console.log("in-body", typeof innerPlain, typeof innerAsync);
}
body();

// Inside its own block an async declaration is perfectly ordinary,
// and it is hoisted there like any other.
{
  const p = later();
  async function later() { return 7; }
  p.then(function (v: any) { console.log("awaited", v); });
}
