// Adapted from test262: console behavior — multi-arg console.log /
// error / warn. Each arg is coerced to string and joined with a single
// space; the whole line gets one trailing newline. Matches JS / bun
// default formatting.
function check(): number {
  // We can't easily assert console output from inside the program;
  // exercise that the dispatch doesn't crash + accepts varied types.
  console.log("x:", 5, "y:", 7);
  console.log(1, 2, 3, 4);
  console.log(true, false);
  console.log("alpha", "beta", "gamma");
  console.error("err:", -1, "ctx:", "fail");

  let xs: number[] = [10, 20, 30];
  console.log("len=", xs.length, "first=", xs[0]);
  return 0;
}
console.log(check());
