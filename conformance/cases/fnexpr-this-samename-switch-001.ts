// A `const g` declared in one `case` binds the whole CaseBlock
// (§14.12.4), so the `g` read in the next clause is that function
// expression, not the outer `g = 5`: falling through from case 0 calls
// it with its receiver (undefined, then `{}` through `.call`), and
// entering at case 1 reads it in its TDZ.

function f(x: number): string {
  const g = 5;
  switch (x) {
    case 0:
      const g = function (this: any, a: number, b: string) {
        return (this === undefined ? "undef:" : "obj:") + a + b;
      };
    case 1:
      return g(7, "s") + g.call({}, 8, "t");
    default:
      return "outer";
  }
}
function h(): number {
  const g = 7;
  return g + 1;
}
console.log(f(0));
try {
  console.log(f(1));
} catch (e) {
  console.log("tdz", e instanceof ReferenceError);
}
console.log(f(2), h());
