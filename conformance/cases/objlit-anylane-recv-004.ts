// RFC 20260717-objlit-anylane-recv knife 2c — a DETACHED
// receiver-first closure (`const g = o.f; g()`) binds
// `this = undefined` per §10.2.1.2 OrdinaryCallBindThis. Pre-fix the
// bare any-call fed the receiver slot from argv, so `g(5)` silently
// bound this = 5 and answered undefined instead of throwing.

const o: any = { v: 7, f() { return this.v; }, plain() { return 100; } };

// detached this-user with no args: this = undefined -> TypeError
const g = o.f;
try {
  g();
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}

// detached this-user WITH an arg: the arg must not become `this`
try {
  g(5);
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}

// detached this-free method keeps working
const h = o.plain;
console.log(h()); // 100

// attached call unaffected
console.log(o.f()); // 7
console.log("done");
