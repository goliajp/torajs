// A callee spelled through a comma or a conditional is no Reference:
// the function runs with an `undefined` receiver, the same as a call
// through its bare name. Function expressions reading `this`, called
// that way in a class method, a plain function and at the top level.

class C {
  tag = 7;
  seq(): any {
    const f = function () {
      return this === undefined ? "undef" : "obj";
    };
    return (0, f)();
  }
  tern(): any {
    const f = function (x: number) {
      return this === undefined ? "undef" + x : "obj";
    };
    const g = function (x: number) {
      return this === undefined ? "U" + x : "O";
    };
    return [(this.tag > 0 ? f : g)(1), (this.tag < 0 ? f : g)(2)].join(",");
  }
  seqCall(): any {
    const f = function () {
      return this.tag;
    };
    return (0, f).call({ tag: 4 });
  }
}

const c = new C();
console.log(c.seq(), c.tern(), c.seqCall());

function nested(): any {
  const f = function () {
    return this === undefined ? "undef" : "obj";
  };
  let n = 0;
  return (n++, n > 0 ? (0, f) : f)() + n;
}
console.log(nested());

function viaApply(): any {
  const f = function (a: number) {
    return this.k + a;
  };
  return (f ? f : f).apply({ k: 2 }, [3]);
}
console.log(viaApply());

// operands, callee, arguments, call: in that order
const log: string[] = [];
const top = function (_a: any) {
  log.push("call");
  return this === undefined ? "undef" : "obj";
};
const r = (log.push("left"), top)((log.push("arg"), 1));
console.log(r, log.join(","));

// a member callee keeps its own receiver rule
const o = {
  v: 3,
  m() {
    return this === undefined ? "undef" : this.v;
  },
};
console.log((0, o.m)(), (0, o).m());

function add(x: number, y: number) {
  return x + y;
}
function mul(x: number, y: number) {
  return x * y;
}
let k = 0;
const z = (k++ > 0 ? add : mul)(k + 1, k + 2);
const w = (k++ > 0 ? add : mul)(k + 1, k + 2);
console.log(z, w, k);
