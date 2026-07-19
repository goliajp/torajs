// expando properties on any-typed arrays shadow builtin methods at the call site
const a: any = [1, 2, 3];
a.join = undefined;
try {
  a.join(",");
  console.log("no throw");
} catch (e: any) {
  console.log("caught:", e instanceof TypeError, String(e.constructor === TypeError));
}
console.log("read face:", a.join);

const b: any = [4, 5];
b.push = 42;
try {
  b.push(6);
  console.log("push no throw");
} catch (e: any) {
  console.log("push caught TypeError:", e instanceof TypeError);
}

const c: any = [1, 2, 3];
c.join = () => "custom";
console.log(c.join());
c.slice = null;
try {
  c.slice(1);
} catch (e: any) {
  console.log("slice null caught:", e instanceof TypeError);
}

const d: any = [9];
d.map = "str";
try {
  d.map((x: any) => x);
} catch (e: any) {
  console.log("map str caught:", e instanceof TypeError);
}
console.log(c.length, d.length);
