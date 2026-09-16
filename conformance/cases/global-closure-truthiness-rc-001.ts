// A module-level variable holding a closure is borrowed when a
// function body tests its truthiness; the test must not release the
// closure the variable still names.
var cf: any;
class K {
  constructor(x: number) {
    if (cf) {
      cf(x);
      return {} as any;
    }
  }
}
function tryNew(label: string) {
  try {
    new K(1);
    console.log(label, "ok");
  } catch (e) {
    console.log(label, "threw", (e as Error).message);
  }
}
function h(x: number): number {
  if (cf) return 1;
  return x;
}
cf = function (x: number) {};
tryNew("s0");
cf = function (x: number) {
  throw new TypeError("t1");
};
tryNew("s1");
cf = function (x: number) {
  throw new TypeError("t2");
};
tryNew("s2");
let n = 0;
for (let i = 0; i < 1000; i++) {
  cf = function (x: number) {
    return x + i;
  };
  n += h(i);
  try {
    new K(i);
  } catch (e) {}
}
console.log("loop", n, cf(1));
