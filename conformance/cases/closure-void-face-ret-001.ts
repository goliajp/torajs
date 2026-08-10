// RFC 20260810-indirect-argc-abi S2b — the TS void-return exception:
// a `() => void` face accepts a value-returning callback (the value
// is ignored), including a generator factory whose call answers a
// generator object. The declared-more-params direction pairs with
// the argc-slot undefined binding.
function assertThrows(f: () => void) {
  try {
    f();
    console.log("no-throw");
  } catch (e) {
    console.log("threw");
  }
}
function* genf(p = 42) {
  yield p;
}
assertThrows(genf);
const it = genf();
console.log(it.next().value);
function voidface(cb: () => void) {
  cb();
}
voidface(() => 7);
voidface((a: any, b: any) => {
  console.log("ab", a === undefined, b === undefined);
});
console.log("done");
