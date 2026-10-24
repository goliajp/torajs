// Shapes that once regressed a test262 case when an async function
// moved off its synchronous lowering: a plain async function reading
// `this` (`.call`'s receiver must arrive), an async body with no await
// at all (it settles its promise inside the call, so a caller's await
// on it takes one job, not more), and a static async method reaching
// a private method through `this`.
async function recv(a: any) {
  return this === a;
}
const o: any = { tag: "o" };
recv.call(o, o).then((v: any) => console.log("recv", v));

const order: string[] = [];
async function noAwait() {
  order.push("body");
  return 1;
}
(async () => {
  order.push("before");
  const r = await noAwait();
  order.push("after " + r);
  console.log(order.join(","));
})();

class C {
  static async #hidden(x: number) {
    return x * 2;
  }
  static async open(x: number) {
    return this.#hidden(x);
  }
}
C.open(21).then((v: any) => console.log("open", v));
