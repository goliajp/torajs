// An object literal of an interface whose method returns an any value,
// handed to a call that leaves trailing parameters out: an optional
// number and a parameter typed `any`. The supplied arguments are
// admitted as a full call's are, so the literal's member stores are
// seen and a class instance passed the same way reaches its row.
const raw: any = JSON.parse('{"x":1.5}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function opt(o: J, k?: number) {
  return o.m();
}
function loose(o: J, k: any) {
  return o.m();
}
function g(): number {
  return 2.5;
}
console.log(opt({ m: g, n: 1 }), opt(new S(raw)), g());
console.log(loose({ m: g, n: 1 }), loose(new S(raw)), loose({ m: g, n: 2 }, 0));
