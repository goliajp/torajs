// Any-method-call RFC 20260704 C3a-2 — dynobj closure property
// calls through the boxed dual entry: native-param / any-param /
// capturing / multi-arg / heap-arg / void-ret closures, plus the
// catchable miss shapes.
const o: any = {
  f: (x: number) => x * 2,
  g: (a: number, b: number) => a + b,
  s: (t: string) => t.length,
  v: 7,
};
console.log(o.f(21));
console.log(o.g(1, 2));
console.log(o.g(1.5, 2.25));
console.log(o.s("hello"));
const base = 100;
const p: any = { add: (x: number) => x + base };
console.log(p.add(5));
const q: any = { echo: (x: any) => x };
console.log(q.echo("str"));
console.log(q.echo(42));
console.log(q.echo(true));
const logs: any = { say: (x: number) => { console.log("say", x); } };
logs.say(3);
try {
  o.v(1);
} catch (e) {
  console.log("non-closure property threw");
}
try {
  o.missing(1);
} catch (e) {
  console.log("missing property threw");
}
