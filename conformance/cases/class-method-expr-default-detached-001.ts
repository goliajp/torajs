// a class method with expression defaults taken as a value and called
// without a receiver: every default evaluates in the callee, in
// parameter order, for a missing argument and for an explicit undefined
// (the receiver-polymorphic twin hosts the bare call and its guards)
let n = 0;
const obj = { k: 1 };
class C {
  m(aFalse = (n += 1, false), aStr = "" + n, aNaN = n += 1, aNull: any = null, aObj = obj) {
    return [aFalse, aStr, Number.isNaN(aNaN) ? "NaN" : aNaN, aNull, aObj === obj, n].join("|");
  }
  static s(x = n += 10, y = "y") { return x + ":" + y + ":" + n; }
}
const c = new C();
const ref = c.m;
const sref = C.s;
console.log(ref(false, "", NaN, null, obj));
console.log(ref(true, "q"));
console.log((ref as any)());
console.log(sref(1), sref(), (sref as any)(undefined, "z"));
console.log(c.m(false, "", 1, null, obj), C.s(5));
