// RFC 20260801-arguments-method-face — fn-value alias
// devirtualization: `var ref; ref = function*() {...}` folds to
// `let ref = Closure __forward_<factory>`, whose declared-arity
// relay dropped every arg (the factory's [...arguments] capture
// arrived empty). An exclusively-called alias now rewrites to
// direct factory calls, so the static-argv face sees the real site
// (test262 gen-func-expr-args-trailing-comma family).
var callCount = 0;
var ref;
ref = function* () {
  console.log(arguments.length);
  console.log(arguments[0]);
  console.log(arguments[1]);
  callCount = callCount + 1;
};
ref(42, null,).next();
console.log(callCount);
