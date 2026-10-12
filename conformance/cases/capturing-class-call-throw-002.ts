// a capturing class constructor called without new throws before its defaults run
function make(tag: string) {
  let log: string[] = [];
  function f(): number {
    log.push("f");
    return 7;
  }
  function boom(): number {
    throw new RangeError("boom");
  }
  class K {
    v: number;
    constructor(x = f(), y = boom()) {
      this.v = x + y + tag.length;
    }
  }
  class L {
    v: number;
    constructor(x: number = f()) {
      this.v = x + tag.length;
    }
  }
  return { K, L, log };
}
const { K, L, log } = make("ab");
const KK: any = K;
const LL: any = L;
try {
  KK();
} catch (e: any) {
  console.log(e instanceof TypeError, e instanceof RangeError, log.length);
}
try {
  LL();
} catch (e: any) {
  console.log(e instanceof TypeError, log.length);
}
try {
  new KK(1);
} catch (e: any) {
  console.log(e instanceof RangeError, e.message, log.length);
}
console.log(new KK(1, 2).v, new LL().v, log.length);
