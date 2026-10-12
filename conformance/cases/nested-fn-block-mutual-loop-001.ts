// Two function declarations of one block call each other, and a loop
// enters the block afresh each iteration: each entry makes new
// bindings (§14.2.3), so a function that escapes the loop keeps the
// iteration it was made in.

const fs: Array<() => number> = [];
for (let i = 0; i < 3; i++) {
  {
    function even(n: number): boolean { return n === 0 ? true : odd(n - 1); }
    function odd(n: number): boolean { return n === 0 ? false : even(n - 1); }
    function get(): number { return even(i) ? i * 10 : i; }
    fs.push(get);
  }
}
for (const f of fs) console.log(f());

function outer(): Array<() => string> {
  const out: Array<() => string> = [];
  for (const tag of ["a", "b"]) {
    {
      function first(): string { return tag + second(); }
      function second(): string { return "!" + tag; }
      out.push(first);
    }
  }
  return out;
}
for (const f of outer()) console.log(f());

function* gen() {
  let base = 100;
  yield 0;
  {
    function ping(n: number): number { return n <= 0 ? base : pong(n - 1); }
    function pong(n: number): number { return ping(n - 1) + 1; }
    yield ping(4);
    base = 200;
    yield pong(3);
  }
}
for (const v of gen()) console.log(v);
