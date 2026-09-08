// §10.2.1.1 fills a constructor's arguments object from argumentsList
// alone, and §9.4.2 GetNewTarget reads a hidden channel of the
// execution context. tr's factory writes `__cm_C__ctor(__this,
// __torajs_my_class_ref("C"), …)`, and the arguments face counted
// that second slot as a user argument — so every constructor's
// arguments object carried one phantom leading entry whose value was
// the class object itself.
class P {
  constructor(a: any, b: any) {
    console.log("P", arguments.length, arguments[0], arguments[1]);
  }
}
new P(1, 2);
new P("x", "y");

class C extends P {
  constructor() {
    super(3, 4);
  }
}
new C();

class Q {
  constructor(a: any) {
    console.log("Q", arguments.length, arguments[0], typeof arguments[0]);
  }
}
new Q(7);

// an ordinary method's face is unchanged
class M {
  m(a: any) {
    console.log("M", arguments.length, arguments[0], arguments[2]);
  }
}
new M().m(8, 9, 10);
