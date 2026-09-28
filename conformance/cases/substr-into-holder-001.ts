// A substring view — a character read or a split element — stored into
// a holder that outlives it: a class field, a generator's lifted local,
// a string global, a resolved Promise. The field and the Promise took
// the view as it was, so the value read another string's bytes once the
// view's source was gone; a string global refused the program.
function churn(): void {
  const j: string[] = [];
  for (let i = 0; i < 50; i++) j.push("vvvvvvvvv" + i);
}

class B {
  r: string = "";
  c: string[] = [];
}
function mk(n: number): B {
  const b = new B();
  const s = "ab-x" + n + ",cd";
  b.r = s.charAt(1);
  b.c = s.split(",");
  return b;
}
const b = mk(3);
churn();
console.log(b.r, b.r.length, b.c[0].split("-").length, b.c[0].slice(0, 2), b.c[1]);

function* gen(s: string) {
  const c = s.charAt(0);
  yield 1;
  yield c + c.length;
}
const it = gen("q" + 5);
it.next();
churn();
console.log(it.next().value);

let g = "";
function setG(s: string): void {
  g = s.charAt(1);
}
setG("xy" + 1);
churn();
console.log(g);

function later(n: number): Promise<string> {
  const d = ("p" + n + ",z").split(",");
  return Promise.resolve(d[0]);
}
const p = later(9);
churn();
p.then((v) => console.log(v, v.length));
