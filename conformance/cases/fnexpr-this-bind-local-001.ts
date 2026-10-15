// A function expression reading `this`, bound to a receiver of the
// program's choosing inside a function body or a class method: the
// bound `this` is the receiver the body reads (§20.2.3.2).

function plain(): any {
  const f = function () {
    return this.tag;
  };
  return f.bind({ tag: 5 })();
}
console.log(plain());

function withCapture(k: number): any {
  const f = function (a: number) {
    return this.tag + k + a;
  };
  return f.bind({ tag: 5 }, 1)();
}
console.log(withCapture(10));

function viaArrow(): any {
  const f = function () {
    return this.tag;
  };
  const g = () => f.bind({ tag: 8 });
  return g()();
}
console.log(viaArrow());

function argumentsSeen(): any {
  const f = function (a: number, b: number) {
    return this.t + arguments.length + a + b;
  };
  const h = f.bind({ t: 1 }, 9);
  return [h(8), h.length].join(",");
}
console.log(argumentsSeen());

class C {
  tag = 7;
  run(): any {
    const f = function () {
      return this.tag;
    };
    const b = f.bind({ tag: 5 });
    return [b(), f.bind(this)()].join(",");
  }
  identity(k: number): any {
    const o = { t: k };
    const f = function () {
      return [this === o, this.t + k].join(":");
    };
    return f.bind(o)();
  }
  partial(): any {
    const f = function (x: number, y: number) {
      return this.m * x + y;
    };
    const h = f.bind({ m: 3 }, 2);
    return [h(1), h(4)].join(",");
  }
  async later(): Promise<any> {
    const f = function () {
      return this.v;
    };
    return f.bind({ v: 6 })();
  }
}

const c = new C();
console.log(c.run(), c.identity(2), c.partial());

// the same name bound in two functions
function first(): any {
  const twin = function () {
    return this.t;
  };
  return twin.bind({ t: 1 })();
}
function second(): any {
  const twin = function () {
    return this.t;
  };
  return twin.bind({ t: 2 })();
}
console.log(first(), second());

c.later().then((v) => console.log(v));
