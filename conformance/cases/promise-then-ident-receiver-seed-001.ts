// A `.then` handler on a binding annotated `Promise<T>`, or on a call
// of a function whose return type is written `Promise<T>`, receives a T
// (ECMA-262 §27.2.5.4; the reaction job passes it the one settled
// value, §27.2.2.1). The value must arrive intact through the typed
// parameter: a fraction, a string, an object field. A second parameter
// receives undefined. Where the spelling does not name the settled
// value (a union of promises, a subclass, `Promise<any>`), or the name
// is bound more than once in the program, nothing is assumed.
const half: Promise<number> = Promise.resolve(2.5);
half.then((v) => console.log("half", v * 2, v + 0.25));

const word: Promise<string> = Promise.resolve("tora");
word.then((v) => console.log("word", v.length, v + "!"));

const pt: Promise<{ x: number; y: number }> = Promise.resolve({ x: 1.5, y: -2 });
pt.then((v) => console.log("pt", v.x + v.y));

half.then((v, w) => console.log("extra", v, w, typeof w));

function src(n: number): Promise<number> {
  return Promise.resolve(n / 4);
}
src(3).then((v) => console.log("declared", v, v * 4));

const mixed: Promise<number | string> = Promise.resolve("s");
mixed.then((v) => console.log("mixed", typeof v, v));

const loose: Promise<any> = Promise.resolve({ k: 7 });
loose.then((v) => console.log("loose", v.k));

class Later extends Promise<number> {}
const sub: Later = Later.resolve(4) as Later;
sub.then((v) => console.log("sub", v + 1));

const either: Promise<number> | Promise<string> = Promise.resolve("e");
either.then((v: any) => console.log("either", v));

function a(): void {
  const q: Promise<string> = Promise.resolve("s");
  q.then((v) => console.log("a", v + "!"));
}
function b(): void {
  const q: Promise<number> = Promise.resolve(7.5);
  q.then((v) => console.log("b", v * 2));
}
a();
b();

half
  .then((v) => v + 1)
  .then((v) => console.log("chain", v));
