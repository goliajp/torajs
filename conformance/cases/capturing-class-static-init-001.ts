// 394-05 — a capturing class's static blocks and this-reading static
// field initializers now route: the emit wraps each into
// `(function () { … }).call(K)`, handing the body the class object
// §15.7.14 binds. Field/block interleaving keeps source order
// (§15.7.10), and a plain initializer stays the bare assignment.

function make(b: number) {
  class K {
    static base = b;
    static f = (this as any).base + 2;
    static tally = 0;
    static {
      (this as any).tally = (this as any).base * 10;
    }
    m() {
      return b;
    }
  }
  return K;
}
const K = make(5) as any;
console.log(K.base, K.f, K.tally);
console.log(new K().m());

// interleaving in source order, reading the class by name
function make2(b: number) {
  class W {
    static x = b;
    static {
      W.x = W.x * 10;
    }
    static y = W.x + 1;
  }
  return W;
}
const W = make2(3) as any;
console.log(W.x, W.y);

// the bare `this` spelling
function make3(c: number) {
  class V {
    static base = c;
    static f = this.base + 2;
  }
  return V;
}
const V = make3(7) as any;
console.log(V.base, V.f);
