// `switch (true)` whose case values branch while they evaluate
// (`typeof` on an `any`, a short-circuit): each clause is compared
// with the value its expression ends with (ES §14.12.4).

function kind(y: any): string {
  switch (true) {
    case typeof y === "string":
      return "string";
    case typeof y === "number" && y > 10:
      return "big number";
    case typeof y === "number":
      return "number";
    default:
      return "other";
  }
}

function notString(y: any): number {
  switch (false) {
    case typeof y === "string":
      return 1;
    default:
      return 0;
  }
}

console.log(kind("a"), kind(50), kind(3), kind(true), kind(null));
console.log(notString("a"), notString(1));
