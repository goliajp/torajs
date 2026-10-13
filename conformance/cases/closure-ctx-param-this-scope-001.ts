// A generator's locals, and an awaiting async function's, are kept on
// that function's own state object. An arrow pushed into one of them
// takes its parameter types from that function's declaration, no
// matter how many other such functions the program has: the receiver
// used to resolve to whichever one was declared last.
function* first(): Generator<number> {
  yield 1;
  const fs: Array<(x: number) => number> = [];
  fs.push((x) => x + 1);
  yield fs[0](41);
}

function* second(): Generator<string> {
  yield "a";
  const gs: Array<(s: string) => string> = [];
  gs.push((s) => s.toUpperCase());
  yield gs[0]("b");
}

class Holder {
  hs: Array<(n: number) => number> = [];
  add(): void {
    this.hs.push((n) => n * 3);
  }
}

async function third(): Promise<void> {
  const ks: Array<(n: number, m: number) => number> = [];
  ks.push((n, m) => n - m);
  await 0;
  console.log(ks[0](10, 4));
}

for (const x of first()) console.log(x);
for (const x of second()) console.log(x);
const h = new Holder();
h.add();
console.log(h.hs[0](5));
third();
