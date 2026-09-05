// A destructuring parameter binds through the synthesized let prelude
// rather than through the parameter name, so the same alias that a
// plain parameter drops survived here and answered the remembered
// class for every argument.
let C: any = class Inner {
  static t = "i";
};
function g({ C }: any) {
  return C.t;
}
console.log(g({ C: { t: "obj" } }));

let D: any = class Inner {
  static t = "i";
};
const h = ([D]: any) => D.t;
console.log(h([{ t: "arr" }]));

let E: any = class Inner {
  static t = "i";
};
const obj = {
  m({ E }: any) {
    return E.t;
  },
};
console.log(obj.m({ E: { t: "method" } }));

let F: any = class Inner {
  static t = "i";
};
class Holder {
  v: any;
  constructor({ F }: any) {
    this.v = F.t;
  }
}
console.log(new Holder({ F: { t: "ctor" } }).v);

// A destructured name elsewhere still leaves the binding's own reads
// intact.
const K = class {
  static t = "k";
};
function unrelated({ K }: any) {
  return K;
}
console.log(unrelated({ K: 1 }), K.t, new K() instanceof K);
