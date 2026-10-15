// A function expression reading `this` whose binding is also tested
// for truthiness (`if (f)`, `f ? a : b`, `!f`, `f && g`) still takes
// the receiver each call gives it: a test calls nothing.

class C {
  tag = 7;
  ternary(): any {
    const f = function (x: number) {
      return this === undefined ? "undef" + x : "obj";
    };
    return f ? f(1) : f(2);
  }
  ifTest(): any {
    const f = function () {
      return this === undefined ? "undef" : this.tag;
    };
    let out = "";
    if (f) out += f();
    if (!f) out += "never";
    while (f && out.length < 12) out += "," + f.call({ tag: out.length });
    return out;
  }
}

const c = new C();
console.log(c.ternary());
console.log(c.ifTest());

function plain(): any {
  const f = function () {
    return this === undefined ? "undef" : this.k;
  };
  const g = function () {
    return this === undefined ? "U" : "O";
  };
  return [f || g ? f() : 0, !g ? 0 : g(), f.call({ k: 3 })].join(" ");
}
console.log(plain());

const top = function () {
  return this === undefined ? "undef" : this.v;
};
for (let i = 0; top && i < 2; i++) console.log(i, top(), top.call({ v: i }));
