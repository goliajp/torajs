// `ArrowFunction : ArrowParameters[?Yield, ?Await] => ConciseBody` —
// the parameters inherit the grammar's `[Await]` parameter, the body
// does not. So inside a class static block (§15.7.1 turns `[Await]`
// on) `(x = await) => 0` is a SyntaxError while
// `() => { var await = 1 }` is not, and a FunctionExpression written
// in the same place is legal either way: its parameters are
// `FormalParameters[~Yield, ~Await]`, unparameterized.
//
// This fixture holds the LEGAL side of that line — over-rejection is
// what a bit that leaks too far would cause, and no negative test
// would notice.

var log: string[] = [];

class A {
  static {
    // ConciseBody is not parameterized: `await` is an ordinary name
    // inside an arrow's body even here.
    var inBody = () => {
      var await = 1;
      return await + 1;
    };
    log.push("arrow-body " + inBody());

    // Ordinary parameters are untouched.
    var plain = (a: number, b = 10) => a + b;
    log.push("arrow-plain " + plain(1));

    // A FunctionExpression's parameters are unparameterized, so the
    // exact spelling the arrow is rejected for stays legal here.
    // Never called: the default would evaluate a name nothing binds,
    // and it is the PARSE that is under test.
    var fnExpr = function (x = await) {
      return x;
    };
    log.push("fn-expr " + typeof fnExpr);

    // A nested async arrow turns `[Await]` on for its own parameters
    // and its body both; nothing here uses the word.
    var asyncArrow = async (n: number) => n * 2;
    log.push("async-arrow " + typeof asyncArrow);
  }
}

// Outside any of that, a sloppy script leaves `await` an ordinary
// name, in an arrow's parameters as anywhere else.
function sloppyHost() {
  var f = (await = 7) => await;
  return f();
}
log.push("sloppy-arrow " + sloppyHost());

for (var i = 0; i < log.length; i++) {
  console.log(log[i]);
}
