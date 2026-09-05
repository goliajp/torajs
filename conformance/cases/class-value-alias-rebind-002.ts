// A formal parameter binds the spelling for a whole body the alias
// would otherwise be read from, so the alias has to drop there too.
// Without the drop the body read the class the alias remembered and
// ignored the argument entirely.
let C: any = class Inner {
  static t = "i";
};
function g(C: any) {
  return C.t;
}
console.log(g({ t: "p" }));

// Same for an arrow, a method and a constructor parameter.
let D: any = class Inner {
  static t = "i";
};
const h = (D: any) => D.t;
console.log(h({ t: "arrow" }));

let E: any = class Inner {
  static t = "i";
};
const obj = {
  m(E: any) {
    return E.t;
  },
};
console.log(obj.m({ t: "method" }));

let F: any = class Inner {
  static t = "i";
};
class Holder {
  v: any;
  constructor(F: any) {
    this.v = F.t;
  }
  read(F: any) {
    return F.t;
  }
}
console.log(new Holder({ t: "ctor" }).v, new Holder({ t: "x" }).read({ t: "m" }));

// A catch parameter is an ordinary lexical binding and shadows the
// same way.
let G: any = class Inner {
  static t = "i";
};
try {
  throw { t: "c" };
} catch (G) {
  console.log((G as any).t);
}

// Dropping the alias must leave the dynamic path answering the same
// thing the alias did — a parameter somewhere else in the program is
// not allowed to cost the binding its own reads.
const K = class {
  static t = "k";
  static self() {
    return K;
  }
};
function unrelated(K: any) {
  return K;
}
console.log(unrelated(1), K.t, K.self() === K, new K() instanceof K);
