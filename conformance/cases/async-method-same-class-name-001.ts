// A top-level class's synchronous method stays synchronous when a class
// of the same name declared in a function has an async method of the
// same name: a method is async by its declaration, not by its class's
// name.

class C {
  m() { return 1 }
  static s() { return "s" }
}
function f() {
  class C {
    async m() { await 0; return 2 }
    static async s() { await 0; return "as" }
  }
  return Promise.all([new C().m(), C.s()]);
}
console.log(new C().m(), C.s());
f().then((v) => console.log("nested", v.join(","), new C().m(), C.s()));
