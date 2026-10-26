// An optional interface method, filled by a class whose method returns
// an any value, read as a value through the interface: the hierarchy
// keeps the return it declares, and the read hands on the method.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

interface K {
  p?(): number;
  q: number;
}
class T implements K {
  v: any;
  q = 1;
  constructor(v: any) {
    this.v = v;
  }
  p(): number {
    return this.v.x;
  }
}
function read(k: K): number {
  const h = k.p;
  return h ? h() : -1;
}
function call(k: K): number {
  return k.p ? k.p() : -1;
}
const arrow: K = { p: () => 4.5, q: 2 };
const bare: K = { q: 3 };
const ks: K[] = [new T(raw), arrow, bare];
for (const k of ks) console.log(call(k), k.q);
console.log(read(arrow), read(bare), typeof new T(raw).p());
