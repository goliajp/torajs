class E2 extends Error {
  constructor(m: string = "") {
    super(m);
    this.name = "E2";
  }
}
function assert2(actual: boolean, msg: string = ""): void {
  if (!actual) {
    throw new E2(msg);
  }
}
function check2<T>(actual: T, expected: T): boolean {
  if (actual !== expected) {
    return actual !== actual && expected !== expected;
  }
  const a: any = actual;
  if (typeof a === "number" && a === 0) {
    const e: any = expected;
    return 1 / a === 1 / e;
  }
  return true;
}
function same2<T>(actual: T, expected: T, msg: string = ""): void {
  if (!check2(actual, expected)) {
    throw new E2(msg);
  }
}
var captured: any = 0;
var C = function () {
  captured = this;
};
same2(C, C, "m");
console.log("ok");
