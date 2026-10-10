// A class whose number fields the program only ever writes integers
// into still takes fractions through an any value cast into one of its
// slots: the cast copies runtime data, so those fields are not narrowed
// to integers (RFC 20260922 §4.4 item 1).
class Q {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
class P {
  a: number;
  tags: number[];
  q: Q | null;
  constructor(a: number) {
    this.a = a;
    this.tags = [1];
    this.q = null;
  }
}
const p0 = new P(1);
console.log(p0.a + 1, p0.tags.length);
const p: P = JSON.parse('{"a":1.5,"tags":[2.5,3],"q":{"v":0.25}}');
console.log(p.a, p.tags[0], p.tags[1], p.q!.v);
const src: any = { a: 2.75, tags: [], q: null };
let p2: P = new P(3);
p2 = src;
console.log(p2.a, p2.tags.length);
function mk(o: any): P {
  return o;
}
console.log(mk({ a: 0.5, tags: [1.5], q: { v: 9 } }).tags[0]);
const ps: P[] = [];
ps.push(src);
console.log(ps[0].a + ps.length);
class Holder {
  p: P;
  constructor() {
    this.p = new P(4);
  }
}
const h = new Holder();
h.p = JSON.parse('{"a":-0.125,"tags":[],"q":null}');
console.log(h.p.a);
