// RFC 20260725 (rotation 206) — a plain `function` expression in an
// object-literal field position binds `this` to the call-site
// receiver, exactly like the method shorthand it sugars.
const m = { n: 7, get: function () { return this.n * 2; } };
console.log(m.get());

// with params
const q = { v: 3, add: function (x: number) { return this.v + x; } };
console.log(q.add(10));
console.log(q.add(0));

// var declaration form
var d = { w: 5, twice: function () { return this.w + this.w; } };
console.log(d.twice());

// this-free fn-expr fields keep the plain closure ABI: field call,
// value read, bare call
const p = { m: function () { return 1; }, k: 2 };
console.log(p.m());
const g = p.m;
console.log(g());

// shorthand and fn-expr coexist on one literal
const both = {
  base: 10,
  sh() { return this.base + 1; },
  fe: function () { return this.base + 2; },
};
console.log(both.sh());
console.log(both.fe());

// mutation seen through this
const c = { count: 0, bump: function () { this.count = this.count + 1; return this.count; } };
console.log(c.bump());
console.log(c.bump());
console.log(c.count);
