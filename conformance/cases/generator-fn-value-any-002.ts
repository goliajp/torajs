// Generator-EXPRESSION const bindings ride the wrapped closure lane
// (r292 blade 2b: the let-init wrap axis admits a hoisted genexpr
// init without the named-fn-refs read gate — top-level reads box
// into any, which a raw FnSig can't). NamedEvaluation still answers
// through the fn-name registry.
const xGen = function* x() {
  yield 1;
};
const gen = function* () {
  yield 2;
};

// fn-name-gen family — NamedEvaluation of the binding
console.log(gen.name);
console.log(xGen.name);
console.log(gen.name !== "xGen");

// the harness read that used to panic (any-boxed argument)
function take(v: any) {
  console.log(typeof v);
}
take(gen);
take(xGen);

// descriptor faces
const d = Object.getOwnPropertyDescriptor(gen, "name");
console.log(d && d.value, d && d.writable, d && d.enumerable, d && d.configurable);

// the factories still mint working generators
console.log(gen().next().value, xGen().next().value);
