// `new Promise(executor)` is a promise of the T its context names
// (ECMA-262 §27.2.3.1): the written `Promise<T>` return of the function
// returning it, an annotated binding, an assertion, or a type argument.
// The settled value must arrive whole through every reaction: then,
// catch, finally, await, Promise.all, a chain, adoption of another
// promise or a thenable, a rejection, a throwing executor, and a
// non-callable executor, which throws synchronously (step 2). Where no
// context names T, or T is a type parameter, nothing is assumed, and
// both kinds of promise can sit in one program.
function half(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
half(5).then((v) => console.log("half", v, v * 4));

function word(): Promise<string> {
  return new Promise((r) => r("tora"));
}
word().then((v) => console.log("word", v.length, v + "!"));

function flag(n: number): Promise<boolean> {
  return new Promise((r) => r(n > 2));
}
flag(3).then((v) => console.log("flag", v, !v));

const bound: Promise<number> = new Promise((r) => r(0.75));
bound.then((v) => console.log("bound", v + 0.25));

const asserted = new Promise((r) => r("as")) as Promise<string>;
asserted.then((v) => console.log("asserted", v + v));

const typeArg = new Promise<number>((r) => r(1.5));
typeArg.then((v) => console.log("type-arg", v * 2));

function adopt(n: number): Promise<number> {
  return new Promise((r) => r(Promise.resolve(n + 0.25)));
}
adopt(1).then((v) => console.log("adopt", v));

function thenable(): Promise<number> {
  return new Promise((r) => r({ then(k: (x: number) => void) { k(3.5); } }));
}
thenable().then((v) => console.log("thenable", v));

function refuse(n: number): Promise<number> {
  return new Promise((r, j) => {
    if (n > 3) r(n);
    else j(new Error("small " + n));
  });
}
refuse(1).catch((e) => console.log("refuse", e.message));
refuse(9).finally(() => console.log("finally")).then((v) => console.log("after", v));

function boom(): Promise<number> {
  return new Promise(() => {
    throw new Error("boom");
  });
}
boom().then(
  (v) => console.log("never", v),
  (e) => console.log("boom", e.message),
);

function notCallable(): Promise<number> {
  return new Promise(7 as any);
}
try {
  notCallable();
} catch (e) {
  console.log("not callable", e instanceof TypeError);
}

function outer(): Promise<number> {
  function inner(): Promise<string> {
    return new Promise((r) => r("inner"));
  }
  return new Promise((r) => inner().then((s) => r(s.length + 0.5)));
}
outer().then((v) => console.log("nested", v));

half(1)
  .then((v) => v + 1)
  .then((v) => console.log("chain", v));

const untyped = new Promise((r) => r({ k: 7 }));
untyped.then((v: any) => console.log("untyped", v.k));

function id<T>(x: T): Promise<T> {
  return new Promise((r) => r(x));
}
id(4.75).then((v) => console.log("generic", v));

console.log("instance", half(2) instanceof Promise);

async function main(): Promise<void> {
  const a = await half(9);
  const s = await word();
  const all = await Promise.all([half(2), half(4)]);
  console.log("await", a + 0.5, s + "?", all[0] + all[1]);
  try {
    await refuse(0);
  } catch (e) {
    console.log("await-reject", (e as Error).message);
  }
}
main();
