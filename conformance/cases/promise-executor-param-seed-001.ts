// The executor of `new Promise(...)` receives the two resolving
// functions of §27.2.1.3 (ECMA-262 §27.2.3.1 steps 8-9): `resolve`
// takes a value or a thenable, `reject` a reason, and any further
// parameter receives undefined. Each must arrive whole however the
// executor uses it: called directly, with no argument or two, with a
// promise or a plain thenable, aliased and called later, stored and
// called from outside. The promise's type comes from a type argument,
// an annotated binding, or the return type of the function returning
// it; a generic return and no context at all type nothing.
function half(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
half(5).then((v) => console.log("half", v));

function adopt(n: number): Promise<number> {
  return new Promise((r) => r(Promise.resolve(n + 0.25)));
}
adopt(5).then((v) => console.log("adopt", v));

function thenable(): Promise<number> {
  return new Promise((r) => r({ then(k: (x: number) => void) { k(3.5); } }));
}
thenable().then((v) => console.log("thenable", v));

function gate(n: number): Promise<number> {
  return new Promise((r, j) => {
    if (n > 3) r(n + 0.5);
    else j(new Error("small " + n));
  });
}
gate(5).then((v) => console.log("gate", v));
gate(1).catch((e) => console.log("gate-err", e.message));

const k = new Promise<number>((r) => r(1.25));
k.then((v) => console.log("type-arg", v));

const word: Promise<string> = new Promise((r) => r("tora"));
word.then((v) => console.log("binding", v.length));

function later(n: number): Promise<number> {
  return new Promise((r) => {
    const alias = r;
    queueMicrotask(() => alias(n * 1.5));
  });
}
later(3).then((v) => console.log("alias", v));

const kept: ((v: number) => void)[] = [];
const outside: Promise<number> = new Promise((r) => {
  kept.push(r);
});
outside.then((v) => console.log("stored", v));
kept[0](6.25);

const done: Promise<void> = new Promise((r) => r());
done.then((v) => console.log("void", v));

new Promise((r) => r()).then((v) => console.log("bare", v));
new Promise((r) => r(1, 2)).then((v) => console.log("two", v));
new Promise((r, j, extra) => {
  console.log("extra", typeof extra);
  r(0);
});
new Promise((r) => {
  throw new Error("boom");
}).catch((e) => console.log("throw", e.message));

function id<T>(x: T): Promise<T> {
  return new Promise((r) => r(x));
}
id(4.75).then((v) => console.log("generic", v));
id("g").then((v) => console.log("generic", v));

let lateOk: (v: number) => void = (_v: number) => {};
let lateErr: (e: number) => void = (_e: number) => {};
const untyped = new Promise((r, j) => {
  lateOk = r;
  lateErr = j;
});
untyped.then((v) => console.log("late", v));
lateOk(7);
lateErr(8);
