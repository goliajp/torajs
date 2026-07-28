// S2.40 — `ClassElement : ;` (ES §15.7): bare semicolons inside a
// class body are empty elements, in declarations and expressions,
// repeated, and between every member shape (the t262 elements/
// suites end each class body with one — a 474-case parse wall).
class D {
  ;
  m() {
    return 42;
  }
  ;;
  f = 3;
  ;
  static s() {
    return 8;
  }
  ;
}
const d = new D();
console.log(d.m(), d.f, D.s());
var C = class {
  n() {
    return 7;
  }
  ;
};
console.log(new C().n());
class E {
  ;
}
console.log(new E() instanceof E);
