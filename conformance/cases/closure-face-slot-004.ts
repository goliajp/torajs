// An `any` value stored into a fn-typed field. A slot that passes no
// argument and discards the return calls it directly whatever its face;
// one that passes arguments or reads the return calls it boxed.
const mk2 = (k: number) => (y?: number) => (y === undefined ? 5 : y) + k;
type T = (y?: number) => number;
let counter = 0;

class K {
  cb: () => void = () => {};
  constructor(x: any) {
    this.cb = x;
  }
  tick() {
    this.cb();
  }
}
const k = new K(() => {
  counter++;
  return "x";
});
k.tick();
k.tick();
console.log(counter);

class K2 {
  cb: (n: number) => void = (n: number) => {};
  constructor(x: any) {
    this.cb = x;
  }
  tick() {
    this.cb(4);
  }
}
const k2 = new K2((n: number, m?: number) => {
  counter += n;
  return m;
});
k2.tick();
console.log(counter);

class K3 {
  f: T = mk2(0);
  constructor(x: any) {
    this.f = x;
  }
}
const k3 = new K3(mk2(1));
console.log(k3.f(), k3.f(2));

function plain(y?: number): number {
  return (y ?? 1) * 10;
}
const o: { f: T } = { f: plain };
console.log(o.f());
o.f = mk2(3);
console.log(o.f(), o.f(1));
