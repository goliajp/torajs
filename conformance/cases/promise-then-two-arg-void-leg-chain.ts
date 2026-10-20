// A two-handler then whose onErr returns nothing derives a promise of
// "the number or undefined"; the next then / catch / await must take it.
Promise.resolve(1)
  .then((v: number) => v * 2, (e: any) => { console.log("e0", e); })
  .then((v: any) => console.log("ok", v), (e: any) => console.log("e", e));
Promise.resolve(1)
  .then((v: number) => v * 2, (e: any) => { console.log("e1", e); })
  .then(() => console.log("done"));
Promise.reject("r")
  .then((v: number) => v * 3, (e: any) => { console.log("caught", e); })
  .then((v: any) => console.log("after reject", v), (e: any) => console.log("e2", e));
Promise.reject("s")
  .then((v: number) => v + 1, (e: any) => { console.log("caught", e); })
  .then(() => console.log("zero-param after reject"));
Promise.resolve(5)
  .then((v: number) => v + 1, (e: any) => { console.log(e); })
  .catch((e: any) => console.log("never", e))
  .then((v: any) => console.log("through catch", v));
Promise.reject("t")
  .then((v: number) => v - 1, (e: any) => { console.log("caught", e); })
  .then((v: number | undefined) => console.log("typed leg", v), (e: any) => console.log("e3", e));
Promise.resolve(9)
  .then((v: number) => v - 1, (e: any) => { console.log(e); })
  .then((v: number | undefined) => console.log("typed leg", v), (e: any) => console.log("e4", e));

// onOk only throws: the derived promise is onErr's string or a rejection.
function boom(v: number): never {
  throw new Error("boom " + v);
}
Promise.resolve(4)
  .then(boom, (e: any) => "fallback " + e)
  .then((s: string) => console.log("len", s.length), (e: any) => console.log("rejected", e.message));
Promise.reject("x")
  .then(boom, (e: any) => "fallback " + e)
  .then((s: string) => console.log("str", s.toUpperCase()));

async function main() {
  const a = await Promise.resolve(7).then((v: number) => v * 10, (e: any) => { console.log(e); });
  console.log("await ok", a);
  await Promise.reject("z").then((v: number) => v * 10, (e: any) => { console.log("await caught", e); });
  console.log("after await");
}
main();
