// A class that reads a binding from around it rides the ES5 lane, and
// that lane used to refuse the whole class the moment it declared any
// static field. The concern was about the word `this` — a static
// initializer runs at class-evaluation time with `this` bound to the
// class object, so inlining one at the store would silently pick up
// the ENCLOSING receiver. An initializer that never says `this` has
// nothing to lose, and a plain assignment is exactly what §15.7.14
// performs for a field (writable, enumerable, configurable — unlike a
// method, which stays non-enumerable).

function make(b: number): any {
  class K {
    static base: number = b;
    static f: number = K.base + 2;
    static tag: string = "t" + b;
    n: number;
    constructor(q: number) { this.n = q + b }
    inst(): number { return this.n * 2 }
    static s(a: number): any { return (this as any).base + a }
  }
  return K;
}

const A: any = make(10);
console.log(1, A.base, A.f, A.tag);
console.log(2, new A(1).n, new A(1).inst());
console.log(3, A.s(5));

// a field is enumerable, a method is not
console.log(4, JSON.stringify(Object.keys(A)));
console.log(5, JSON.stringify(Object.getOwnPropertyDescriptor(A, "base")));
console.log(6, JSON.stringify(Object.getOwnPropertyDescriptor(A, "s")));

// each evaluation mints its own class, statics included
const B: any = make(100);
console.log(7, B.base, B.f, A.base, A.f, A !== B);

// source order: a later initializer sees an earlier one
function ord(): any {
  class K {
    static a: number = 1;
    static b: number = K.a + 1;
    static c: number = K.b + 1;
  }
  return K;
}
const C: any = ord();
console.log(8, C.a, C.b, C.c);

// a static field holding a function value that captures too
function fnfield(b: number): any {
  class K {
    static g: any = function (x: number): number { return x + b };
  }
  return K;
}
console.log(9, fnfield(7).g(1));

// a capture-free class with statics is untouched by any of this
class Plain {
  static k: number = 3;
  static m(): number { return 4 }
}
console.log(10, Plain.k, Plain.m(), JSON.stringify(Object.keys(Plain)));
