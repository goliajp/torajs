// A `Sub[]` (Sub extends Tok) handed to a `Tok[]` parameter through
// `any` fits and is the caller's own array: the body's push lands a
// plain Tok in it. The holder reads it as an element and releases it
// with the array — sized as the Tok it is, not as a Sub, or the
// allocator hands that block out again under live cells and the
// loop below reads a clobbered receiver.
class Tok {
  kind: number;
  value: number;
  constructor(kind: number, value: number) {
    this.kind = kind;
    this.value = value;
  }
}
class Sub extends Tok {
  extra: number;
  tail: number;
  more: number;
  last: number;
  constructor(value: number) {
    super(1, value);
    this.extra = 7;
    this.tail = 11;
    this.more = 13;
    this.last = 17;
  }
}
class Builder {
  count: number;
  constructor() {
    this.count = 0;
  }
  append(toks: Tok[], value: number) {
    toks.push(new Tok(0, value));
    return toks.length;
  }
}
console.log("typed ->", new Builder().append([new Tok(0, 1)], 2));
const b: any = new Builder();
const subs: Sub[] = [new Sub(1), new Sub(2)];
console.log("push ->", b.append(subs, 30), subs.length, subs[2].value, subs[2].kind, subs[2] instanceof Sub, subs[1].extra);
let total = 0;
const keep: Sub[] = [];
for (let round = 0; round < 2000; round++) {
  const xs: Sub[] = [new Sub(round), new Sub(2)];
  b.append(xs, round);
  total = total + xs[2].value + xs[0].extra;
  keep.push(new Sub(round));
  keep.push(new Sub(round + 1));
}
for (let i = 0; i < keep.length; i++) {
  total = total + keep[i].extra + keep[i].tail + keep[i].more + keep[i].last;
}
console.log("rounds ->", total);
