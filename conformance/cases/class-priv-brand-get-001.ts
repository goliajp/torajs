// §7.3.31 PrivateGet / PrivateBrandCheck — reading a private element
// from an object whose class did not declare it throws TypeError
// (never answers undefined), while a declared field storing
// undefined reads back undefined without throwing.
var C = class {
  #m = "cfield";
  read(o: any) {
    return o.#m;
  }
};
var D = class {
  #u: any = undefined;
  read(o: any) {
    return o.#u;
  }
};
let c: any = new C();
let d: any = new D();
console.log(c.read(c));
console.log("declared-undef:", d.read(d));
try {
  console.log("got:", c.read(d));
} catch (e: any) {
  console.log("caught:", e instanceof TypeError);
}
try {
  console.log("got:", c.read(42));
} catch (e: any) {
  console.log("caught-prim:", e instanceof TypeError);
}
