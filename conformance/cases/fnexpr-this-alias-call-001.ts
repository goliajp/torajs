// A function expression reading `this`, called through a `const`
// alias, gets the plain-call receiver (`undefined`), the same as when
// it is called by its own name — in a plain function, through a chain
// of aliases, in a function called as a method, and in an
// object-literal method. Each function spells its own names: a name
// declared in several functions keeps the receiver promotion off.
function outer() {
  const f1 = function () { return this === undefined ? "undef" : "obj" };
  const g1 = f1;
  return g1();
}

function chained(x: number) {
  const f2 = function (y: number) { return this === undefined ? x + y : -1 };
  const g2 = f2;
  const h2 = g2;
  return [f2(1), g2(2), h2(3)].join(",");
}

const o: any = { tag: 1 };
o.m = function () {
  const f3 = function () { return this === undefined };
  const g3 = f3;
  return g3();
};

const lit = {
  tag: 2,
  m() {
    const f4 = function () { return this === undefined ? "undef" : "obj" };
    const g4 = f4;
    return g4();
  },
};

console.log(outer(), chained(10));
console.log(o.m());
console.log(lit.m());
