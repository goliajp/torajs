// Generator factory fn values ride the wrapped closure lane into any
// positions (r292: the collector's blanket generator exclusion
// predates the G2 forward-cell reflection faces — gen-proto install
// at mint + FLAG_FN_GENERATOR routing). getPrototypeOf keeps its
// bare-Ident compile-time fold (kind-exact), everything else wraps.
function* g() {
  yield 1;
  yield 2;
}
async function* ag() {
  yield 1;
}

// restricted-properties family — gOPD on a generator factory
console.log(Object.getOwnPropertyDescriptor(g, "caller"));
console.log(Object.getOwnPropertyDescriptor(g, "arguments"));
console.log(Object.getOwnPropertyDescriptor(ag, "caller"));

// this-val-not-generator family — fn value as a runtime-lane this
const GeneratorPrototype = Object.getPrototypeOf(g).prototype;
try {
  GeneratorPrototype.next.call(g);
} catch (e) {
  console.log("caught next.call(g)");
}
try {
  GeneratorPrototype.next.call(g, 1);
} catch (e) {
  console.log("caught next.call(g, 1)");
}

// the bare-Ident getPrototypeOf fold stays kind-exact
const gfp = Object.getPrototypeOf(g);
const agfp = Object.getPrototypeOf(ag);
console.log(gfp === Object.getPrototypeOf(function* () {}));
console.log(gfp !== agfp);
console.log(gfp.constructor.name, agfp.constructor.name);

// the factory itself still mints working generators
const it = g();
console.log(it.next().value, it.next().value, it.next().done);
