// An async function declaration that reads `this` moves like any
// other: the body's `this` becomes the generator's leading receiver
// parameter, handed in by the wrapper's own `this` — so a `.call` /
// `.apply` receiver reaches the body, an arrow inside it still sees
// the same receiver, a plain strict call sees `undefined` (rejection,
// not a synchronous throw), and the body suspends at its first await
// (`b` prints before either `c`).
async function f(k: number): Promise<any> {
  console.log("a");
  const v = (this as any).v;
  await null;
  console.log("c");
  const g = () => (this as any).v * 2;
  return v + k + g();
}
f.call({ v: 9 }, 1).then((x: any) => console.log("call", x));
f.apply({ v: 2 }, [3]).then((x: any) => console.log("apply", x));
async function h(): Promise<any> {
  await null;
  return (this as any).v;
}
h().then((x: any) => console.log("plain ok", x), (e: any) => console.log("plain rej", e instanceof TypeError));
console.log("b");
