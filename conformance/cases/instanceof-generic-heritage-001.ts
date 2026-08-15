// `instanceof` against a class with GENERIC descendants (rotation
// 411): a generic class's instances wear per-specialization tags
// the constant descendant chain can never list, so every generic
// class in the answer set — the target itself and every generic
// descendant on the chain — contributes a runtime name-identity
// check. Both receiver lanes: typed (`w`) and any-held (`wa`).
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}
class Wide<U> extends Box<U> {
  constructor(v: U) {
    super(v);
  }
}
class Solid extends Wide<number> {
  constructor() {
    super(5);
  }
}
const w = new Wide<string>("a");
const b = new Box<number>(1);
console.log(w instanceof Wide, w instanceof Box, b instanceof Box, b instanceof Wide);
const wa: any = new Wide<string>("a");
const sa: any = new Solid();
console.log(wa instanceof Box, wa instanceof Wide, sa instanceof Box, sa instanceof Wide, sa instanceof Solid);
console.log(({}) instanceof Box, (5 as any) instanceof Box);
