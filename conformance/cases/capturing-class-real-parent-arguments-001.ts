// a real class parent's constructor reached through a capturing subclass's super(1, 2)
class P {
  n: number;
  t: boolean;
  constructor(a?: number, b?: number) {
    this.n = arguments.length;
    this.t = new.target !== undefined;
  }
}
function make(k: number) {
  class Q extends P {
    k: number;
    constructor() {
      super(1, 2);
      this.k = k;
    }
  }
  return Q;
}
const Q = make(3);
const q = new Q();
console.log(q.n, q.t, q.k, q instanceof P);
