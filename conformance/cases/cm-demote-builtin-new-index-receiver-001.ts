// A method name owned by exactly one user class is rewritten to that
// class's static method before type checking; a receiver that is not an
// instance of the class must fall back to the member call. Built-in
// `new` expressions and element reads were never probed, so they ran
// the user class's body.
class P {
  valueOf() {
    return 7;
  }
  get(k: number) {
    return "P.get" + k;
  }
  m() {
    return "P.m";
  }
}
console.log(new Boolean(0).valueOf(), new Number(3).valueOf(), new String("s").valueOf());
console.log(new Map([[1, "one"]]).get(1));
const xs: any[] = [new Boolean(0), { m() { return "o.m"; } }];
console.log(xs[0].valueOf(), xs[1].m());
const ps: P[] = [new P()];
console.log(ps[0].m(), ps[0].valueOf(), new P().get(2));
class Q {
  push(x: number) {
    return -1;
  }
}
const c = xs.length > 0;
const b: any = new Boolean(0);
console.log(new Q().push(1));
console.log((c ? b : b).valueOf(), (b ?? b).valueOf(), (b || b).valueOf());
