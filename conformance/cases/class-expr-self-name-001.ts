// §15.7.14 step 3 binds a class's own name inside the class scope for
// both halves of the grammar. tr models it for a declaration, where
// the binding is spelled the way the source wrote it; a named class
// EXPRESSION carries a synthesized binding instead, so its body's
// reference to itself used to resolve nowhere.
const K = class C {
  static tag = "k";
  static self() {
    return C;
  }
  who() {
    return C.tag;
  }
};
console.log(K.self() === K, new K().who());

// The inner binding is a scope inside every other, so it shadows a
// class of the same name declared outside.
class C {
  static tag = "outer";
}
const L = class C {
  static tag = "inner";
  static read() {
    return C.tag;
  }
};
console.log(L.read(), C.tag);

// And like the declaration half, it is immutable.
const M = class C {
  static bad() {
    try {
      C = 1 as any;
    } catch (e: any) {
      return e.constructor.name;
    }
    return "no throw";
  }
};
console.log(M.bad());

// A parameter of the same name shadows it, the way any binding does.
const N = class C {
  static via(C: any) {
    return C;
  }
};
console.log(N.via("param"));

// A static field's initialiser runs at class-init time with the
// binding already in place, so it reads the name like a body does.
const S = class C {
  static tag = "s";
  static me = C;
  static read = C.tag;
};
console.log(S.me === S, S.read);
