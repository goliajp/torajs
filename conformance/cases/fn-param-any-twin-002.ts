// Class methods with fn-typed parameters reached through `any`: an
// instance method reading its receiver, and a static method with two
// such parameters. The typed calls keep working alongside.
function nf(v: number) {
  console.log("nf", v);
}
class K {
  base = 10;
  run(f: (v: number) => void) {
    f(this.base + 4);
  }
  static two(f: (v: number) => void, g: (v: number) => void) {
    f(1);
    g(2);
  }
}
const k: any = new K();
k.run((v: number) => console.log("m", v));
k.run(nf);
const ks: any = K;
ks.two(nf, (v: number) => console.log("s", v));
try {
  ks.two(nf, 3);
} catch (e) {
  console.log("not callable", e instanceof TypeError);
}
new K().run(nf);
K.two(nf, nf);
