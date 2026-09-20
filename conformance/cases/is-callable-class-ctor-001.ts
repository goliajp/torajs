// ES §7.2.3 IsCallable — the shapes that carry [[Call]]. tr models a
// class constructor as a dynobj, and a builtin constructor read as a
// value IS one of those, so every hand-rolled copy of this predicate
// that listed only "closure cell" answered `false` for `Error`.
//
// §13.5.3 step 9 defines `typeof` as this predicate, which is why the
// two are checked together: they must never disagree.
console.log(typeof Error, typeof Map, typeof Object);
class C {
  m(): number {
    return 1;
  }
}
console.log(typeof C, typeof (() => 1));
function g(): number {
  return 1;
}
console.log(typeof g, typeof {}, typeof "s");
const px: any = new Proxy(function () {}, {});
console.log(typeof px);
const pxo: any = new Proxy({}, {});
console.log(typeof pxo);

// §20.2.3.5 step 4 raises only when `this` is not callable. A builtin
// constructor is callable, so this must NOT throw — the text it
// answers is checked by the native-form case next door, because tr
// and bun agree on the throw/no-throw question one knife earlier
// than they agree on the spelling.
let threw: boolean = false;
try {
  String(Error);
} catch (e) {
  threw = true;
}
console.log("String(Error) threw:", threw);

// The gate itself still holds for a receiver that really is not
// callable (rotation 680 put Function.prototype on it).
try {
  (Function.prototype as any).toString.call({});
  console.log("call({}) did not throw");
} catch (e) {
  console.log("call({}) threw");
}
