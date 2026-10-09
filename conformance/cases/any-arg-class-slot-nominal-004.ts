// a parameter typed by a class through `T | null`, `T | undefined`,
// `readonly T[]` or a type alias admits, through `any`, only that
// class's cells: an instance of another class with the same fields
// keeps its own methods
class P {
  x = 1;
  m() {
    return "P" + this.x;
  }
}
class Q {
  x = 1;
  m() {
    return "Q" + this.x;
  }
}
type A = P;
function fn(p: P | null) {
  return p === null ? "null" : p.m() + ":" + (p instanceof P);
}
function fu(p: P | undefined) {
  return p === undefined ? "undef" : p.m() + ":" + (p instanceof P);
}
function fr(ps: readonly P[]) {
  return ps.map((p) => p.m()).join();
}
function fa(p: A) {
  return p.m() + ":" + (p instanceof P);
}
function fd(p: P = new P()) {
  return p.m() + ":" + (p instanceof P);
}
const gn: any = fn;
const gu: any = fu;
const gr: any = fr;
const ga: any = fa;
const gd: any = fd;
console.log(gn(new Q()), gn(new P()), gn(null));
console.log(gu(new Q()), gu(new P()), gu(undefined));
console.log(gr([new Q(), new P()]));
console.log(ga(new Q()), ga(new P()));
console.log(gd(new Q()), gd(new P()), gd());
