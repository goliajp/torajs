// A class method reading `arguments` under a spread call: the count is
// a runtime fact, so the site leaves the direct-call form and the
// method takes the runtime argv face. The static expander is not
// consulted here — its index expansion trims the list to the declared
// arity and the count would die with the trimmed tail.
class C {
  none() {
    console.log("none", arguments.length);
  }
  one(a: any) {
    console.log("one", arguments.length, a, arguments[2]);
  }
}

const xs = [1, 2];
const ys = [3, 4];

new C().none(...xs);
new C().none(42, ...xs, ...ys);
new C().one(...xs, ...ys);

const anyRecv: any = new C();
anyRecv.none(...ys);

class D {
  m() {
    console.log("d", arguments.length, arguments[0], arguments[1]);
  }
}
const d = new D();
d.m(...[9].map((n) => n), ...ys);
