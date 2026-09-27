// The clauses of a switch share one scope, and a clause written after
// a `default` sees what the default declares: a closure in that later
// clause captures the default's binding. Both the checker and the
// lowering visited the default last, so the binding was not yet
// declared when the later clause's closure was built and its read
// threw a ReferenceError.
var fromDefault: any, fromCase: any;
switch (7) {
  default:
    let x = "inside";
    fromDefault = function () { return x; };
  case 0:
    fromCase = function () { return x + "!"; };
}
console.log(fromDefault(), fromCase());

function pick(n: number): string {
  let f: any;
  switch (n) {
    case 1:
      let tag = "one";
    default:
      tag = "dflt:" + n;
    case 2:
      f = () => tag;
  }
  return f();
}
console.log(pick(1));
