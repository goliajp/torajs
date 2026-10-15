// A `const f` holding a function expression reading `this` in one
// function does not make another function's `const f` (an arrow) a
// receiver-reading callee: `(c ? f : echo)("s")` there stays a call
// through the union of the two functions, while the method's own
// `(c ? echo : f).call(this)` is still called through each operand.

function echo(x: any) {
  return x;
}
class K {
  tag = "k";
  m(): string {
    const f = function () {
      return this === undefined ? "U" : "O";
    };
    return f.call(this) + (process.argv.length > 99 ? echo : f).call(this);
  }
}
function B(c: boolean) {
  const f = (n: number) => n * 2;
  return (c ? f : echo)("s");
}
console.log(new K().m(), B(false), B(true));
