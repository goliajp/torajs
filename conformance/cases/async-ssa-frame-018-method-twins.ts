// An async method reached with a receiver of another shape runs the
// method's receiver-polymorphic twin (a prototype call, `.call` on a
// plain object). The twin is the same async function: it suspends at
// its awaits, a throw in its body or in a parameter default rejects
// its promise instead of raising at the call, and its value settles
// the promise.
var x = 0;
const log: string[] = [];
class C {
  v = 1;
  async m(x = x) {
    await null;
    return 1;
  }
  async get(k: number) {
    log.push("get-start " + k);
    await null;
    log.push("get-after " + k);
    return this.v + k;
  }
  async boom(k: number) {
    await 0;
    throw new Error("boom " + k + " " + this.v);
  }
  static async s(this: any, k: number) {
    await null;
    return this.tag + k;
  }
}
const viaProto = C.prototype.m();
log.push("after proto call");
viaProto.then(
  () => log.push("proto resolved (wrong)"),
  (e: any) => log.push("proto rejected " + (e.constructor === ReferenceError)),
);
const other: any = { v: 40 };
C.prototype.get.call(other, 2).then((v: any) => log.push("call " + v));
C.prototype.boom.call(other, 3).catch((e: any) => log.push("caught " + e.message));
const st: any = { tag: "t" };
C.s.call(st, 5).then((v: any) => log.push("static " + v));
log.push("main end");
Promise.resolve()
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => console.log(log.join("\n")));
