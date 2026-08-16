// §27.2.5.4 step 9 calls a `then` handler with `Call(handler,
// undefined, «argument»)`, so a function EXPRESSION in that slot reads
// `this` as undefined — but only when the receiver is a SYNTACTICALLY
// certain promise, since a user object with its own `then` decides for
// itself how it calls what it is handed.
//
// Certainty now reads through a mutable binding the program writes
// exactly once, which is what `var p = Promise.resolve(1)` is after
// `desugar_var_hoist`. The chain census is a fixpoint, so a `var`
// written from a link of an already-certain chain qualifies too.

var p: any = Promise.resolve("v");

p.then(function (v: any) {
  console.log("then", v, typeof this);
});

// a chain over a certain promise, bound across statements — `q` only
// qualifies once `p` has, which is what the fixpoint is for
var q: any = p.then(function (v: any) {
  return v + "!";
});

q.catch(function () {
  console.log("unreachable");
});

q.then(function (v: any) {
  console.log("chained", v, this === undefined);
});

// calling an `async function` answers an intrinsic promise (§27.7.5.1
// builds it with the %Promise% capability whatever the body returns),
// so the same bar clears
async function make(): Promise<string> {
  return "async";
}

var r: any = make();

r.finally(function () {
  console.log("finally", typeof this);
});
