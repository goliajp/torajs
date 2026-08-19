// §15.7.14 — a class extending a BUILTIN constructor makes the class
// object's [[Prototype]] that constructor, so builtin statics are
// inherited: `CP.resolve` reads through the chain onto Promise's
// interned static cell.
class CP extends Promise<any> {}
const cp: any = CP;
console.log(typeof cp.resolve, typeof cp.reject, typeof cp.all);
console.log(Object.getPrototypeOf(cp) === Promise);

class CA extends Array<number> {}
const ca: any = CA;
console.log(typeof ca.isArray, ca.isArray([1, 2]));

class CO extends Object {}
const co: any = CO;
console.log(typeof co.keys, co.keys({ a: 1, b: 2 }).length);
