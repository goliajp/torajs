// SuperCall / super-method-call spread arguments (§13.3.7.1
// ArgumentListEvaluation): a static literal spread folds at parse
// time, a dynamic spread desugars via apply_spread_args AFTER the
// class pass rewrote the site into a plain __cm_* call.
class P {
  a: number;
  b: number;
  c: number;
  constructor(a: number, b: number, c: number) {
    this.a = a;
    this.b = b;
    this.c = c;
  }
  sum(...xs: number[]): number {
    let t = this.a + this.b + this.c;
    for (const x of xs) t += x;
    return t;
  }
}
class Q extends P {
  constructor() {
    super(...[1, 2, 3]);
  }
  viaDyn(xs: number[]): number {
    return super.sum(...xs);
  }
}
const q = new Q();
console.log(q.a, q.b, q.c);
console.log(q.viaDyn([10, 20]));
class R extends P {
  constructor(xs: number[]) {
    super(...xs);
  }
}
const r = new R([4, 5, 6]);
console.log(r.a, r.b, r.c);
