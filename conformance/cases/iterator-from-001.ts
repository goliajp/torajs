// RFC 20260730-iterator-global 刀 4 — Iterator.from (§27.1.6.2):
// already-Iterator pass-through identity, builtin iterables mint
// their @@iterator lane, string primitives iterate, plain {next}
// objects wrap (WrapForValidIterator), non-string primitives
// refuse, wrap next/return forward to the underlying.

function* g() {
  yield 1;
  yield 2;
}

// Pass-through: an existing Iterator answers itself.
const it: any = g();
const same: any = Iterator.from(it);
console.log(same === it);

// A helper cell is an Iterator too — same pass-through.
const h0: any = g();
const h: any = h0.map((v: any) => v);
console.log(Iterator.from(h) === h);

// Builtin iterables mint their @@iterator lane.
console.log(JSON.stringify(Iterator.from([10, 20]).toArray()));
const m = new Map();
m.set("a", 1);
console.log(JSON.stringify(Iterator.from(m).toArray()));
const s = new Set();
s.add(7);
s.add(8);
console.log(JSON.stringify(Iterator.from(s).toArray()));

// String primitive iterates per code unit.
console.log(JSON.stringify(Iterator.from("ab").toArray()));

// Plain { next } object wraps lazily; the wrap is an Iterator.
let steps = 0;
const plain: any = {
  next() {
    steps++;
    return steps <= 2
      ? { value: steps * 10, done: false }
      : { value: undefined, done: true };
  },
};
const w: any = Iterator.from(plain);
console.log(steps);
console.log(w instanceof Iterator);
console.log(JSON.stringify(w.next()));
console.log(JSON.stringify(w.map((v: any) => v + 1).toArray()));

// Wrap return() forwards to the underlying's own return.
let closed = 0;
const closable: any = {
  next() {
    return { value: 1, done: false };
  },
  return() {
    closed++;
    return { value: 99, done: true };
  },
};
const w2: any = Iterator.from(closable);
console.log(JSON.stringify(w2.next()));
console.log(JSON.stringify(w2.return()));
console.log(closed);

// A custom @@iterator wins over self-as-iterator.
const custom: any = {
  [Symbol.iterator]() {
    return g();
  },
};
console.log(JSON.stringify(Iterator.from(custom).toArray()));

// Non-string primitives refuse.
const n: any = 5;
try {
  Iterator.from(n);
} catch (e: any) {
  console.log(e instanceof TypeError);
}
