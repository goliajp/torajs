// A captured binding reassigned from per-turn closures, holding an
// array, a string, a closure and a class instance in turn.
let a: number[] = [0];
for (let i = 0; i < 1000; i++) {
  const f = () => { a = [i, i + 1]; };
  f();
}
console.log(a.length, a[0], a[1]);

let s = "start";
for (let i = 0; i < 1000; i++) {
  const f = () => { s = "v" + i; };
  f();
}
console.log(s);

let k: () => number = () => -1;
for (let i = 0; i < 1000; i++) {
  const f = () => { k = () => i * 2; };
  f();
}
console.log(k());

class P { constructor(public t: number) {} }
let p = new P(0);
for (let i = 0; i < 1000; i++) {
  const f = () => { p = new P(i); };
  f();
}
console.log(p.t);

let o: { n: { m: number } } = { n: { m: 0 } };
for (let i = 0; i < 1000; i++) {
  const f = () => { o = { n: { m: i } }; };
  f();
}
console.log(o.n.m);
