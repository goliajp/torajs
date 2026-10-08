// Guards that narrow a union binding from outside an `if` condition:
// a `switch (true)` case, a condition held in a const (TS aliased
// conditions), and an assertion function (`asserts v is T`).

function byCase(y: string | number): number {
  switch (true) {
    case typeof y === "string":
      return y.length;
    case typeof y === "number":
      return y * 2;
    default:
      return -1;
  }
}

function byAlias(y: string | number): string {
  const isStr = typeof y === "string";
  if (isStr) return y.toUpperCase();
  return "n" + y;
}

function assertStr(v: unknown): asserts v is string {
  if (typeof v !== "string") throw new Error("not a string: " + v);
}

function assertSet(v: unknown): asserts v {
  if (!v) throw new Error("unset");
}

function byAssert(y: string | number): number {
  assertStr(y);
  return y.length;
}

function setLen(y: string | null): number {
  assertSet(y);
  return y.length;
}

console.log(byCase("abc"), byCase(4));
console.log(byAlias("abc"), byAlias(4));
console.log(byAssert("hello"), setLen("xy"));
try {
  byAssert(3);
} catch (e) {
  console.log((e as Error).message);
}
try {
  setLen(null);
} catch (e) {
  console.log((e as Error).message);
}
