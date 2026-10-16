// A function expression reading `this`, constructed with `new` in one
// function, while another function declares a `const` of the same
// name holding a number and a third one an object.

function mk(v: number): any {
  const F = function (this: any, n: number) {
    this.v = n;
  };
  return new (F as any)(v);
}
function other(): number {
  const F = 7;
  return F * 2;
}
function third(): string {
  const F = { tag: "t" };
  return F.tag;
}
const o = mk(3);
console.log(o.v, other(), third(), mk(4).v);
