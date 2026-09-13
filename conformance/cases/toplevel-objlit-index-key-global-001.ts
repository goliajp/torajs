// A top-level object literal with an INDEX key (`{ length: 2, 0: 0,
// 1: 2 }`, test262's array-like idiom) was invisible to every named
// function: the all-literal `__inlobj` spelling refuses a non-
// identifier key, and the shared any-promote verdict refused it too,
// so the binding stayed a main-fn local and each read answered
// "unknown identifier". Closures reached it through their env, which
// is why an un-moved async arrow saw it and the same arrow moved into
// a generator (blade 2) did not — Array.fromAsync's mapfn cases. An
// index key is data like any identifier key: the binding promotes as
// an `any` slot, the dynobj holds the key under its canonical string.
const arrayLike = { length: 4, 0: 0, 1: 2, 2: Promise.resolve(4), 3: 6 };
const plain = { length: 2, 0: "a", 1: "b" };
function named() {
  return plain.length + ":" + plain[0] + plain[1];
}
console.log("named", named());
class K {
  m() {
    return arrayLike.length;
  }
}
console.log("method", new K().m());
function* gen() {
  yield plain[1];
  yield arrayLike[3];
}
const it = gen();
console.log("gen", it.next().value, it.next().value);
// unannotated like test262's — an annotated named fn as the mapfn hits
// a different wall (`box_to_any element type FnSig`, registered)
async function asyncMap(val, ix) {
  return Promise.resolve(val * ix);
}
function runner(f: any) {
  return f();
}
runner(async () => {
  const result = await Array.fromAsync(arrayLike, asyncMap);
  console.log("value", result, arrayLike.length);
});
