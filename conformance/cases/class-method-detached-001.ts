// RFC 20260813-detached-objlit-method — a class method read as a
// VALUE. The read already worked (the runtime mints the method value
// off the class-methods table, and `typeof` answered `function`), but
// calling the binding did not: the checker names the method's
// signature there — correctly, TypeScript says the same — while the
// slot holds an Any, so the call went looking for a FuncId by name
// and rejected with "unknown function `t`". `.call` on it rejected as
// an unsupported member-call shape.
class C {
  n = 7;
  read() {
    return this.n;
  }
}

const c = new C();
console.log(c.read());

const t = c.read;
console.log(typeof t);
console.log(t.call(c));
console.log(t.call({ n: 9 }));

// A class body is strict code, so a bare call gets an undefined
// receiver rather than the global object — reading through it throws.
try {
  t();
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
