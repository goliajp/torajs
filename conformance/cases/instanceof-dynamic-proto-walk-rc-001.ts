// The dynamic `instanceof` walk reads an OWNED prototype per hop and
// must hand each one back: repeated walks over every receiver kind,
// mortal prototypes included, with the constructors dropped afterwards.
function mkf(): any { return function () {} }
function mkc(): any { class K { v = 1 } return K }
function mkd(B: any): any { return class extends B {} }
function* gen() { yield 1 }
class Top { t = 1 }
class Sub extends Top {}
let hits = 0;
for (let i = 0; i < 200; i++) {
  const F = mkf(), K = mkc(), D = mkd(K), DD = mkd(D);
  const recvs: any[] = [
    new F(), Object.create(F.prototype), new K(), new D(), new DD(), new Sub(), {}, [], () => 0,
    gen(), new Map(), new Proxy({}, {}), Object.create(null), new Error("e"), /x/, new Date(0), 1, "s", null, undefined,
  ];
  const targets: any[] = [F, K, D, DD, Top, Sub, Object, Function, Array, Map, Error];
  for (const r of recvs) for (const t of targets) if (r instanceof t) hits++;
}
console.log(hits);
const F = mkf(), o: any = new F();
Object.setPrototypeOf(F.prototype, Top.prototype);
console.log(o instanceof F, o instanceof Top, o instanceof Sub, Object.create(o) instanceof Top);
