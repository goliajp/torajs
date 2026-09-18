// A unit class's static field is DEFINED on the evaluation's class
// object (§7.3.34 DefineField = CreateDataPropertyOrThrow), not
// written: `static name` / `static length` replace the non-writable
// function properties, the initializer is a NamedEvaluation, and the
// class object's own keys come in §15.7.14 order.
function mk(): any {
  class K {
    static name = "zz";
    static length = 5;
    static f = () => 1;
    static n = 7;
    static o = { a: 1 };
  }
  return K;
}
const A = mk(), B = mk();
console.log(A === B, A.name, A.length, A.n, JSON.stringify(A.o));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(A, "name")));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(A, "length")));
console.log(typeof A.f, A.f(), A.f.name, A.f === B.f, A.o === B.o);
console.log(Object.getOwnPropertyNames(A).join(","));
console.log(Object.keys(A).join(","));
A.n = 9;
console.log(A.n, B.n);
