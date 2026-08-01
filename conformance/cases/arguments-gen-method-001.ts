// arguments in generator METHODS (RFC 20260801-arguments-method-face
// knife 2b): body arguments idents rename to the trailing
// __torajs_gen_argv param at parse; the class-side forwarder hands
// over [...arguments], expanded to the exact call-site argv by the
// method static-argv face.

// 1. zero-param gen method, over-arity prototype call (the
//    cls-decl-gen-meth-args-trailing-comma family shape).
class C1 {
  *gm1() {
    console.log(arguments.length);
    console.log(arguments[0], arguments[1]);
  }
}
C1.prototype.gm1(42, 'TC39',).next();

// 2. declared param + over-arity, instance receiver.
class C2 {
  *gm2(a) {
    yield arguments.length;
    yield arguments[1];
  }
}
var it2 = new C2().gm2(1, 2);
console.log(it2.next().value, it2.next().value);

// 3. bare escape inside a gen method body.
class C3 {
  *gm3() {
    var xs = arguments;
    yield xs.length;
  }
}
console.log(new C3().gm3(9, 10).next().value);

// 4. async generator method.
class C4 {
  async *gm4() {
    yield arguments.length;
  }
}
C4.prototype.gm4(5, 6).next().then(function (r) {
  console.log(r.value);
});

// 5. reads across yield resumptions.
class C5 {
  *gm5() {
    yield arguments[0];
    yield arguments[1];
  }
}
var it5 = new C5().gm5(7, 8);
console.log(it5.next().value, it5.next().value);
