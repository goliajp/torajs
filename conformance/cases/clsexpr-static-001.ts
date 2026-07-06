// RC-3 (RFC 20260706-test262-bug-corpus): class-expression bindings —
// static method calls route through the P8.5 alias onto the named-
// class static machinery, and the alias now covers var/let bindings
// (with reassignment / rebinding dropping it). The `var C = class {
// static method([x, y]) {…} }; C.method([1, 2])` shape below is the
// test262 language/expressions/class/dstr meth-static form that used
// to throw "value is not a function on this any receiver".
const C = class {
  static method([x, y]: number[]): number {
    return x + y;
  }
};
console.log(C.method([1, 2]));
var D = class {
  static tag(): string {
    return "D";
  }
  greet(): void {
    console.log("hi from D");
  }
};
console.log(D.tag());
new D().greet();
let callCount = 0;
var E1 = class {
  static bump(): void {
    callCount = callCount + 1;
  }
};
E1.bump();
E1.bump();
console.log(callCount);
