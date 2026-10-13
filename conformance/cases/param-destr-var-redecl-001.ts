// §15.2.1 early errors set a function's parameter names against the
// body's LexicallyDeclaredNames only: a `var` may re-declare a
// parameter, a destructured one included, and §10.2.11 step 28 makes
// it the parameter's own binding (an init-less `var` leaves the value
// alone, an initializer overwrites it).

function plain({ a }: any) { var a; return a; }
console.log("plain", plain({ a: 1 }));

function withInit({ a }: any) { var a = 5; return a; }
console.log("with-init", withInit({ a: 1 }));

function arrayPat([x, y]: any) { var y; var x = x + 10; return x + ":" + y; }
console.log("array", arrayPat([1, 2]));

function later({ a }: any) { a = a + 1; var a; return a; }
console.log("later", later({ a: 3 }));

class C {
  m({ a }: any) { var a; return a; }
  n({ b }: any) { var b = b * 2; return b; }
}
console.log("method", new C().m({ a: 2 }), new C().n({ b: 4 }));

const arrow = ({ a }: any) => { var a; return a; };
console.log("arrow", arrow({ a: 7 }));
