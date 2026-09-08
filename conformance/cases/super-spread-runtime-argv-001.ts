// super(...) with a runtime-width argument list reaches a parent
// constructor that reads `arguments` (rotation 616).
var source = [3, 4, 5];
var target;

class P {
  constructor() {
    console.log("len", arguments.length);
    console.log("vals", arguments[0], arguments[1], arguments[2], arguments[3], arguments[4]);
  }
}

class A extends P {
  constructor() { super(...source) }
}
class B extends P {
  constructor() { super(1, 2, ...source) }
}
class C extends P {
  constructor() { super(1, 2, ...target = source) }
}
class D extends P {
  constructor() { super() }
}

new A();
new B();
new C();
new D();
console.log("target is source", target === source);
