// A method-carrying top-level object literal promotes under __inlobj
// with the receiver-less __mth field spelling, so named-fn reads and
// calls resolve — the rotation-238 tb2 shape (this-state method) and
// its parameterized siblings.

const iter = {
  count: 0,
  next() {
    this.count++;
    return this.count;
  },
};
function drive() {
  console.log(iter.next());
  console.log(iter.next());
  console.log(iter.count);
}
drive();
console.log(iter.next());
console.log(iter.count);

// user parameters stay in the signature; the receiver stays out
const acc = {
  v: 10,
  add(n: number) {
    this.v += n;
    return this.v;
  },
  scale(f: number, g: number) {
    this.v = this.v * f + g;
    return this.v;
  },
};
function bump() {
  console.log(acc.add(5));
  console.log(acc.scale(2, 1));
}
bump();
console.log(acc.v);

// the test262 iterator idiom: method returning an object literal
const seq = {
  i: 0,
  next() {
    this.i++;
    return { done: this.i > 2, value: this.i };
  },
};
function pull() {
  console.log(seq.next().value);
  console.log(seq.next().value);
  console.log(seq.next().done);
}
pull();
