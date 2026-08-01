// RFC 20260801-arguments-method-face — a stored method-value escape
// (`var ref = C.method`) reroutes through a synthesized __forward_*
// relay whose declared-arity call used to break the uniform-argc
// gate, dropping the fn to the Real tier (length right, values
// undefined — silent wrong). The relay site now stays out of the
// argc vote (test262 cls-*-meth-static-args-trailing-comma family).
var callCount = 0;
class C {
  static method() {
    console.log(arguments.length, arguments[0], arguments[1]);
    callCount = callCount + 1;
  }
}
C.method(42, "TC39",);
var ref = C.method;
console.log(callCount);

// named-fn variant of the same escape shape
function tail() {
  console.log(arguments[0], arguments[1], arguments.length);
}
tail("a", "b",);
var alias = tail;
console.log(typeof alias);
