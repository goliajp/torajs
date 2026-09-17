// f64 handlers named through an immutable closure binding: the value
// reaching the parameter and the value the handler returns keep their
// fractional part through promise chains and element callbacks.
function f() {
  const half = (v: number): number => v + 0.5;
  console.log([1.5, 2.5].map(half));
  const show = (v: number): void => { console.log("s " + v); };
  [1.5].forEach(show);
  function call(cb: (v: number) => number): number { return cb(1.5); }
  console.log(call(half));
  const g = half;
  console.log(g(0.25));
  Promise.resolve(1.5).then(half).then((v: number) => console.log("p " + v));
}
f();
const topHalf = (v: number): number => v + 0.5;
Promise.resolve(1.5).then(topHalf).then((v: number) => console.log("top " + v));
const halfB = (v: number): number => v + 0.5;
Promise.resolve(1.5).then(halfB).then((v: number) => console.log("a " + v));
const dblB = (v: number): number => v * 2;
Promise.resolve(3).then(dblB).then((v: number) => console.log("b " + v));
const showB = (v: number): void => { console.log("c " + v); };
Promise.resolve(1.5).then(showB);
function namedB(v: number): number { return v + 0.5; }
Promise.resolve(1.5).then(namedB).then((v: number) => console.log("d " + v));
const kB = 0.25;
const capB = (v: number): number => v + kB;
Promise.resolve(1.5).then(capB).then((v: number) => console.log("e " + v));
function inner2() {
  const h2 = (v: number): number => v + 0.5;
  Promise.resolve(1.5).then(h2).then((v: number) => console.log("f " + v));
  const onErr = (e: any): number => 0.75;
  (new Promise<number>((_o, no) => no(1))).then((v: number) => v, onErr).then((v: number) => console.log("g " + v));
}
inner2();
