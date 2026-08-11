// RFC 20260810-indirect-argc-abi S1-T1 — declared-param methods on
// the runtime argv face: the hidden sig argc rides `__cm_` bodies
// too, and the S2 missing-argument normalization keys on it. An
// under-arity escape call must bind undefined into the missing Any
// params (the invoke buffer's undefined padding and S2 agree);
// over-arity reads ride the true argv.

// under-arity: b binds undefined, beyond-argc reads answer undefined
class H {
  mH(a, b) {
    console.log(arguments.length, a, b, arguments[2]);
  }
}
var refH = H.prototype.mH;
refH(1);
refH(1, 2, 3);

// zero-argument call: every declared param binds undefined
class K {
  mK(x) {
    console.log(arguments.length, x, arguments[1]);
  }
}
var refK = K.prototype.mK;
refK();
refK("a", "b");
