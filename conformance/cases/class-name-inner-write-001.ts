// The class scope binds the class's own name immutably (§15.7.14
// ClassDefinitionEvaluation step 3), and every method body sits
// inside that scope — so a write to it from in there is a runtime
// TypeError, not something the compiler may refuse. The RHS still
// evaluates first (§13.15.2 takes rref before PutValue throws).
let sideEffects = 0;
function rhs(): any {
  sideEffects++;
  return 1;
}

class C {
  static v = 7;
  static s() {
    try {
      C = rhs();
    } catch (e: any) {
      console.log("static " + e.constructor.name + ": " + e.message);
    }
  }
  m() {
    try {
      C = rhs();
    } catch (e: any) {
      console.log("method " + e.constructor.name + ": " + e.message);
    }
  }
}

C.s();
new C().m();
console.log("rhs ran", sideEffects);
console.log("C survived", C.v, typeof C);
