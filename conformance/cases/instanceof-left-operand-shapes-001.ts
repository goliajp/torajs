// The left operand of instanceof in every binding shape: catch
// parameter, parameter, closure capture, global, and inside an async
// function, holding a function, an object or a primitive. The operator
// only reads it, so each value must survive the test intact.

class Box {
  n: number;
  constructor(n: number) { this.n = n; }
}

function decl() { return 1; }
const fnExpr = function () { return 2; };
const obj = new Box(3);
const prim: any = 4;
const plain: any = { k: 5 };

function check(label: string, x: any): void {
  console.log(label, x instanceof Box, x instanceof Function, x instanceof Object);
}

// parameter
check("param fn", decl);
check("param fnExpr", fnExpr);
check("param obj", obj);
check("param prim", prim);
check("param plain", plain);

// global
console.log("global", decl instanceof Function, obj instanceof Box, prim instanceof Box);

// closure capture
function captured(): () => string {
  const f = decl;
  const o = obj;
  const n: any = 6;
  return () => [f instanceof Function, o instanceof Box, n instanceof Box].join(",");
}
const read = captured();
console.log("capture", read(), read());

// catch parameter
for (const thrown of [new Box(7), new Error("e"), 8, decl] as any[]) {
  try {
    throw thrown;
  } catch (e) {
    console.log("catch", e instanceof Box, e instanceof Error, e instanceof Function);
  }
}

// owned left operands: a call result, a member read, a function expression
function make(): any { return new Box(9); }
const holder: any = { inner: new Box(10), fn: decl };
console.log("owned", make() instanceof Box, holder.inner instanceof Box,
  holder.fn instanceof Function, (function () {}) instanceof Function);

// a target with no @@hasInstance that is not callable throws
try {
  console.log(decl instanceof ({} as any));
} catch (e) {
  console.log("throws", e instanceof TypeError);
}

// inside an async function, across an await
async function inAsync(x: any): Promise<string> {
  const before = x instanceof Box;
  await null;
  return [before, x instanceof Box, decl instanceof Function].join(",");
}
inAsync(obj).then((s) => {
  console.log("async", s);
  console.log("intact", obj.n, decl(), fnExpr(), holder.inner.n, plain.k);
});
