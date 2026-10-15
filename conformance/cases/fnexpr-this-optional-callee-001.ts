// A function expression reading `this`, called through `f?.(x)` or
// `(a ?? f)(x)`: the receiver is `undefined` (the callee is a binding,
// or no Reference at all), and `??` picks its right side only for a
// nullish left side, before the arguments evaluate.

const log: string[] = [];
function arg(s: string) {
  log.push(s);
  return s;
}

const f = function (x: string) {
  return (this === undefined ? "undef:" : "obj:") + x;
};
console.log(f?.(arg("a")));

let h: any = undefined;
console.log((h ?? f)(arg("b")));
h = (x: string) => "h:" + x;
console.log((h ?? f)(arg("c")));

let g: ((x: string) => string) | undefined = undefined;
console.log((g ?? f)("d"));
g = (x: string) => "g:" + x;
console.log((g ?? f)("e"));

function plain(): string {
  const p = function () {
    return this === undefined ? "U" : "O";
  };
  let n: any = null;
  return p?.() + (n ?? p)();
}
console.log(plain());

class C {
  tag = 1;
  m(): string {
    const p = function () {
      return this === undefined ? "U" : "O";
    };
    const q: any = this.tag > 0 ? undefined : p;
    return p?.() + (q ?? p)();
  }
}
console.log(new C().m());
console.log(log.join(","));
