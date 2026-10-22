// A handler with a typed parameter on a promise of `any` is handed
// whatever the promise settled with (§27.2.5.4). When that value does
// not fit the parameter the handler runs untyped, so what it returns is
// not its declared return type: the derived promise settles `any`, with
// one handler or two. A two-handler `.then` whose handlers return a
// string or undefined settles exactly that union, and the next handler
// reads undefined as undefined, not null.

async function main() {
  const x: any = "str";
  const a = await Promise.resolve(x).then((v: number) => v + 1);
  console.log("one", typeof a, a);
  const b = await Promise.resolve(x).then((v: number) => v + 1, () => 0);
  console.log("two", typeof b, b);
  const n: any = 4;
  const c = await Promise.resolve(n).then((v: number) => v * 2, () => 0);
  console.log("fits", typeof c, c);
  Promise.resolve(x)
    .then((v: number) => v + 1, () => 0)
    .then((w) => console.log("chain", typeof w, w));

  const q = Promise.resolve(1).then((v) => (v > 5 ? "big" : undefined), () => "err");
  q.then((v) => console.log("undef", v === undefined, v === null, typeof v));
  const r = Promise.resolve(9).then((v) => (v > 5 ? "big" : undefined), () => "err");
  r.then((v) => console.log("str", v === undefined, typeof v, v === undefined ? 0 : v.length));
  const s = Promise.reject(new Error("e")).then((v) => "ok", () => undefined);
  s.then((v) => console.log("reject leg", v === undefined, typeof v));
}
main();
