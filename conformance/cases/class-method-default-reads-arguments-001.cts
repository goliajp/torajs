// A class method whose parameter defaults read `arguments` beyond the
// declared arity: called through the prototype or as a static, the
// call's whole argv reaches the defaults (the test262 shape, as a
// script: a class expression, one call site per method).
var C = class {
  method(x = arguments[2], y = arguments[3], z) {
    console.log(x, y, z);
  }
  static smethod(x = arguments[2], y = arguments[3], z) {
    console.log(x, y, z);
  }
};
C.prototype.method(undefined, undefined, "third", "fourth");
C.smethod(undefined, undefined, "third", "fourth");
