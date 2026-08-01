// arguments method face knife 1 (RFC 20260801-arguments-method-face):
// single-owner class methods join the static-argv face - over-arity
// values reach the body via injected extras, length folds to the
// uniform call-site argc, escapes swap to the materialized array.

// 1. zero-param method, over-arity prototype call (the
//    cls-decl-meth-args-trailing-comma family shape).
var callCount = 0;
class C {
  method() {
    console.log(arguments.length);
    console.log(arguments[0], arguments[1]);
    callCount = callCount + 1;
  }
}
C.prototype.method(42, 'TC39',);
console.log(callCount);

// 2. declared params + over-arity, instance receiver call.
class D {
  dm(a, b) {
    console.log(arguments.length, a, b, arguments[2]);
  }
}
new D().dm(1, 2, 3);

// 3. under-arity: length answers the call-site argc, missing param
//    reads undefined.
class E {
  em(a, b, c) {
    console.log(arguments.length, a, b, c);
  }
}
new E().em(9);

// 4. bare escape from a method body rides the materialized array.
class F {
  fm() {
    var xs = arguments;
    console.log(xs.length, xs[0] + xs[1]);
  }
}
F.prototype.fm(4, 5);

// 5. length write inside a method takes the LiveLength lane.
class G {
  gm(a) {
    arguments.length = 3;
    console.log(arguments.length, a);
  }
}
new G().gm(8);
