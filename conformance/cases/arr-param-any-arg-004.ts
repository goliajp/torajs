// A shifted array handed to a `Tok[]` parameter through `any`: shift
// moves the deque head and leaves the shifted-off pointers in the
// slots below it, so the layout test has to read the live elements
// from the head on. Here the tail is a plain `Base`, which does not
// fit `Tok`, while the slots below the head still hold live `Tok`s:
// a test that started at slot 0 would pass the array to the typed
// body, which reads `.value` past the end of the `Base`.
class Base {
  kind: number;
  constructor(kind: number) {
    this.kind = kind;
  }
}
class Tok extends Base {
  value: number;
  constructor(kind: number, value: number) {
    super(kind);
    this.value = value;
  }
}
class Reader {
  base: number;
  constructor(base: number) {
    this.base = base;
  }
  last(toks: Tok[]) {
    return toks[toks.length - 1].value;
  }
}
const r: any = new Reader(100);
console.log("typed ->", new Reader(100).last([new Tok(0, 8)]));
const a = new Tok(0, 1);
const b = new Tok(0, 2);
const xs: Base[] = [a, b, new Tok(0, 3), new Base(7)];
console.log("unshifted ->", r.last(xs), xs.length);
xs.shift();
console.log("shifted ->", r.last(xs), xs.length);
xs.shift();
console.log("shifted twice ->", r.last(xs), xs.length, a.value + b.value);
