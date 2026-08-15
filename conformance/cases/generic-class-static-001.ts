// A generic class's static method (rotation 411): desugar threads
// the class's type params onto `__sm_<C>__<m>`, but a static never
// mentions them (that is a TS error), so the call site has nothing
// to infer T from — an unmentioned type param binds `any` (TS
// infers `unknown` there and admits the call). Inherited-static
// dispatch (`NumBox.make`) rides the same record.
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  static make(n: number): number {
    return n * 2;
  }
}
class NumBox extends Box<number> {
  constructor() {
    super(1);
  }
}
console.log(Box.make(3), NumBox.make(5));
