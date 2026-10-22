// An async function that writes no return type settles its promise with
// whatever its returns give (ECMA-262 §27.7.5.3 AsyncFunctionStart,
// §27.2.1.3.2). When every path returns one primitive type, a handler
// or an `await` reads that type; the value must arrive whole: a
// fraction, a string, a boolean. A path off the end settles with
// undefined, returns of two types settle with either, a returned
// promise or thenable is adopted, and a throw rejects.
async function one() {
  return 1.5;
}
one().then((v) => console.log("one", v * 2));

async function word(n: number) {
  if (n > 1) return "big";
  return "small";
}
word(2).then((v) => console.log("word", v.length, v));
word(0).then((v) => console.log("word", v.length, v));

async function flag(n: number) {
  return n > 2;
}
flag(3).then((v) => console.log("flag", v, typeof v));

async function sum(a: number, b: number) {
  return a + b;
}
sum(0.25, 0.5).then((v) => console.log("sum", v));

async function maybe(n: number) {
  if (n > 1) return 3;
}
maybe(0).then((v) => console.log("maybe", v, typeof v));
maybe(5).then((v) => console.log("maybe", v));

async function mixed(n: number) {
  if (n > 1) return 1;
  return "s";
}
mixed(0).then((v) => console.log("mixed", typeof v, v));
mixed(2).then((v) => console.log("mixed", typeof v, v));

async function adopt() {
  return Promise.resolve(4.5);
}
adopt().then((v) => console.log("adopt", v));

async function thenable() {
  return { then(k: (x: number) => void) { k(8.5); } };
}
thenable().then((v) => console.log("thenable", v));

async function thrower(n: number) {
  if (n > 0) throw new Error("t" + n);
  return 2;
}
thrower(1).catch((e) => console.log("thrower", e.message));
thrower(0).then((v) => console.log("thrower-ok", v + 0.5));

async function awaited() {
  const x = await Promise.resolve(2);
  return x + 1;
}
awaited().then((v) => console.log("awaited", v));

async function main() {
  const a = await one();
  const w = await word(5);
  console.log("await", a + 1, w.toUpperCase());
}
main();
