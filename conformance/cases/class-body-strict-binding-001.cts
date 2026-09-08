// §15.7.1 — all parts of a class are strict mode code, whatever the
// goal says. §13.1.1 then refuses `eval` and `arguments` as BINDINGS
// there, while a plain sloppy-Script function may still bind them.
//
// This fixture guards the half that a positive test can show: what is
// still ALLOWED. Over-rejecting is the dangerous direction, and the
// refusals themselves are covered by test262 (a rejected program has
// no stdout to compare).
function sloppyMayBind(eval) {
  return eval + 1;
}
console.log(sloppyMayBind(41));

var arguments = "a plain sloppy var may be named this";
console.log(typeof arguments);

class C {
  // Reading them is legal in strict code; only binding is refused.
  reads() {
    return typeof eval + "," + arguments.length;
  }
  // An ordinary parameter name is unaffected by §15.7.1.
  ordinary(x) {
    return x * 2;
  }
}
const c = new C();
console.log(c.reads());
console.log(c.ordinary(21));
