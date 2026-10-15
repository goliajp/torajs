// The names a named class expression (§15.7.14 step 3) and a named
// function expression (§15.2.5) bind for their own bodies, and the
// scope a parameter default reads (§10.2.11), as seen by `new`.

class X {
  tag = "outer class";
}

function outerFn() {
  function X(this: any) {
    this.tag = "outer fn";
  }
  const W = class X {
    tag = "inner class";
    static make() {
      return new X();
    }
  };
  return W.make().tag;
}
console.log(outerFn());

const V = class X {
  tag = "inner class 2";
  static make() {
    return new X();
  }
};
console.log(V.make().tag);

class F {
  tag = "class F";
}

function viaFnExpr() {
  const g = function F(n: number): any {
    if (n > 0) return new F(n - 1);
    return { tag: "fn F" };
  };
  return g(1).tag;
}
console.log(viaFnExpr());

class P {
  tag = "class P";
}

function viaDefault() {
  function P(this: any) {
    this.tag = "fn P";
  }
  function g(x: any = new P()) {
    return x.tag;
  }
  return g();
}
console.log(viaDefault());
