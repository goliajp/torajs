// A property written on a named function AFTER the function was read as
// a value lands on the same bag every face reads. The value read mints
// the canonical cell and binds the fn's props storage to it; the cell's
// props slot is still empty then, and the FnSig-spelled store
// (`foo.x = 1`) used to hand a null bag to `dynobj_set`, which writes
// nothing — `foo.x`, `Object.keys(foo)`, an HOF `thisArg`'s field and
// a class's `super.<static>` all answered undefined (rotation 637).
function foo() {}
var z: any = foo;
foo.x = 1;
console.log(Object.keys(foo).length, foo.x, z.x, Object.keys(z).length);
function callbackfn(val, idx, obj) { return this.res; }
function thisArg() {}
var t: any = thisArg;
thisArg.res = true;
console.log([1].every(callbackfn, thisArg), [1, 2].some(callbackfn, t));
function Parent() {}
var p: any = Parent;
Parent.test262 = "t";
class C extends Parent { static v = super.test262; }
console.log(C.v, p.test262);
