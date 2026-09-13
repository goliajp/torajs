// RFC 20260805-async-fn-state-machine blade 1 / 3 — shapes the move
// gate refuses, each of which regressed a test262 case when moved
// (rotation 641 close sweep): a plain async function reading `this`
// (the driver's thunk calls the generator plainly, so `.call`'s
// receiver would be lost), an async body with no await at all (no
// suspend point to gain; moving reorders the settle microtask), and a
// static async method reaching a private method through `this`.
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
