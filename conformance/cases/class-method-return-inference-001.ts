// r502 — a class method with no return annotation infers its return
// type from the body the way a fn declaration does (TS infers both):
// the shape sniff now resolves `this.<field>` against the class's
// declared field rows, so `sum()` below is a number on the typed lane
// instead of an `any` box at every call site. The fall-through method
// keeps the `any` floor (a reachable end of body answers undefined —
// no scalar slot can spell that), and every value below must agree
// with bun byte for byte.
class Vec3 {
  x = 1;
  y = 2;
  z = 3;
  label = "v";
  flag = true;
  sum() {
    return this.x + this.y + this.z;
  }
  scaled(k: number) {
    return this.sum() * k;
  }
  name() {
    return this.label + "!";
  }
  on() {
    return this.flag && this.x > 0;
  }
  pick(c: boolean) {
    return c ? this.x : this.y;
  }
  maybe(c: boolean) {
    if (c) {
      return this.z;
    }
  }
  twice() {
    if (this.flag) {
      return this.sum() * 2;
    }
    return 0;
  }
}
class Vec4 extends Vec3 {
  w = 4;
  total() {
    return this.sum() + this.w;
  }
}
const v = new Vec3();
console.log(v.sum(), v.scaled(10), v.name(), v.on(), v.pick(true), v.pick(false));
console.log(v.maybe(true), v.maybe(false), v.twice());
const u = new Vec4();
console.log(u.total(), u.scaled(2), u.name());
let acc = 0;
for (let i = 0; i < 500; i++) {
  const t = new Vec4();
  t.w = i;
  acc += t.total();
}
console.log(acc);
