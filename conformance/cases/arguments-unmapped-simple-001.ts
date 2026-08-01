// Module code is strict (ES §10.4.4.6/7): the arguments object is
// UNMAPPED for every fn — simple params included, not just
// default/rest/destructured ones. Neither direction may alias:
// an arguments write must not reach the param, and a param write
// must not show in a later arguments read. Mirrors test262
// unmapped/via-strict + 10.6-10-c-ii-1-s.
function foo(a: number, b: string, c: number) {
  arguments[0] = 1;
  arguments[1] = "str";
  arguments[2] = 2.1;
  console.log(a, b, c);
  console.log(arguments[0], arguments[1], arguments[2]);
}
foo(10, "sss", 1);

function bar(x: number) {
  x = 42;
  console.log(arguments[0]);
  console.log(arguments.length);
}
bar(7);
