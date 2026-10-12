// Declarations of a generator body lifted to top level before the
// machine splits it: their signatures keep naming the body's own
// `type` / `interface` (which shadow top-level ones of the same name),
// and a declaration deeper in the body, or nested in a peer, still
// reaches the lifted one after a `yield`.

type Num = string;
interface Rec { a: string; b: number }

function* shadowed() {
  type Num = number;
  interface Rec { b: number; a: string; c: boolean }
  function add(x: Num, y: Num): Num { return x + y; }
  function mk(): Rec { return { b: 5, a: "five", c: true }; }
  function show(r: Rec): string { return r.a + ":" + r.b + ":" + r.c; }
  yield 0;
  yield add(1, 2);
  yield show(mk());
  const r = mk();
  yield r.c;
}
for (const v of shadowed()) console.log(v);
const top: Rec = { a: "t", b: 1 };
const s: Num = "s";
console.log(top.a, top.b, s);

function* deeper() {
  function f(): number { return 1; }
  function g(): number {
    function inner(): number { return f() + 10; }
    return inner();
  }
  yield 0;
  {
    function h(): number { return f() + 100; }
    yield h();
  }
  yield g();
}
for (const v of deeper()) console.log(v);
