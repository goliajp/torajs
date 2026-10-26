// Two classes declare a field of the same name. A store of an any value
// through a receiver typed as the first widens its field; an instance
// of the second is stored into that field, so it is handed on only once
// the field holds values of any kind, a round after the store is found.
// From there it reaches a receiver typed as the first class, and the
// store writes the second class's own field, which its method reads.
class N {
  p: N | null;
  constructor() {
    this.p = this;
  }
  self(): boolean {
    return this.p === this;
  }
}
class Holder {
  p: N | null;
  constructor(n: N) {
    this.p = n;
  }
}
function st(h: Holder, v: any): void {
  h.p = v;
}
const raw: any = JSON.parse('{"q": 1}');
const n = new N();
const h = new Holder(n);
const x: Holder = h.p!;
st(x, raw);
console.log(n.self(), n.p === raw);
st(h, raw);
console.log(h.p === raw);
