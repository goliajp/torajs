// fn-value `.apply` with a spread-carrying LITERAL argArray
// (§20.2.3.1 + §13.3.8.1, rotation 372): the spread lane re-enters
// with the array's elements as the argument list; the inline
// fn-expr receiver joins the boxed-only argv face (real argc/argv
// through the closure cell's boxed adapter).
let target: any;
const source = [3, 4, 5];
(function () {
  console.log(arguments.length, arguments[2], arguments[4], target === source);
}).apply(null, [1, 2, ...(target = source)]);

const src2 = [7, 8];
(function () {
  console.log("b", arguments.length, arguments[0], arguments[3]);
}).apply(null, [1, 2, ...src2]);

(function () {
  console.log("empty", arguments.length);
}).apply(null, [...[]]);

// a closure-value binding rides the same route
const f = function () {
  return arguments.length;
};
console.log("cv", f.apply(undefined, [1, ...src2]));

// named fn + literal argArray with spread — the desugar swallows it
// into a direct spread call the static expander takes
function n3(a: number, b: number, c: number): string {
  return `${a}|${b}|${c}`;
}
const nsrc = [2, 3];
console.log("named", n3.apply(null, [1, ...nsrc]));
