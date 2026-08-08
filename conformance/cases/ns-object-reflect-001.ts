// RFC 20260801-ns-object-value (Reflect extension) — the Reflect
// namespace object as a first-class value: thisArg identity, the
// @@toStringTag badge, escaped-singleton calls (get / has / ownKeys
// / construct through an any binding), and the reflection lengths of
// the four newly tabled statics.
function cb(this: any) {
  return this === Reflect;
}
console.log([11].every(cb, Reflect));
console.log(Object.prototype.toString.call(Reflect));
console.log(Object.getPrototypeOf(Reflect) === Object.prototype);
var R: any = Reflect;
var t: any = { a: 7 };
console.log(R.get(t, "a"));
console.log(R.has(t, "a"), R.has(t, "b"));
console.log(R.ownKeys(t).length, R.ownKeys(t)[0]);
console.log(
  Reflect.get.length,
  Reflect.has.length,
  Reflect.ownKeys.length,
  Reflect.construct.length,
);
console.log(Reflect.get.name, Reflect.construct.name);
class C {
  v: number;
  constructor(x: number) {
    this.v = x;
  }
}
var inst: any = R.construct(C, [42]);
console.log(inst.v, inst instanceof C);
console.log(Object.keys(Reflect).length);
