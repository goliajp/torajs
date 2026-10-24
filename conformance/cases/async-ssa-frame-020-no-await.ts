// An async function whose body never awaits runs to its end inside
// the call (§27.7.5.2 AsyncBlockStart) and its promise settles there:
// a returned value fulfils it, a throw (a parameter default's
// included) rejects it, and a returned promise or thenable is adopted
// two jobs later (§27.2.1.3.2 NewPromiseResolveThenableJob, then its
// `then` reaction), so the result is never the returned promise.
const log: string[] = [];
const tick = (n: number) => {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) p = p.then(() => log.push("t" + i)) as any;
};
async function num(n: number): Promise<number> {
  log.push("num body " + n);
  return n * 2;
}
async function str(s: string): Promise<string> {
  return s + "!";
}
async function voided(): Promise<void> {
  log.push("void body");
}
async function thrower(k: number): Promise<number> {
  if (k > 0) throw new Error("thrown " + k);
  return k;
}
var x = 0;
async function badDefault(x = x) {
  return 1;
}
const inner = Promise.resolve(7);
async function adopt(): Promise<number> {
  return inner;
}
async function thenable(): Promise<any> {
  return { then(r: any) { log.push("then called"); r(9); } };
}
async function arr(): Promise<number[]> {
  return [1.5, 2.5];
}
async function untyped(a) {
  return a + a;
}
class K {
  v = 3;
  async get(k: number) {
    return this.v + k;
  }
  static async make(k: number) {
    return new K().v * k;
  }
}
const arrow = async (k: number) => k + 100;
const fnExpr = async function (k: number) {
  if (k < 0) throw new RangeError("neg");
  return k;
};
tick(4);
num(1).then((v) => log.push("num " + v));
log.push("after num call");
str("s").then((v) => log.push("str " + v));
voided().then((v) => log.push("void " + v));
thrower(2).catch((e: any) => log.push("thrower " + e.message));
thrower(0).then((v) => log.push("thrower ok " + v));
badDefault().then(
  () => log.push("badDefault resolved (wrong)"),
  (e: any) => log.push("badDefault " + (e.constructor === ReferenceError)),
);
const adopted = adopt();
log.push("adopt is inner " + (adopted === inner));
adopted.then((v) => log.push("adopt " + v));
thenable().then((v: any) => log.push("thenable " + v));
arr().then((v: any) => log.push("arr " + v[0] + " " + v[1]));
untyped(4).then((v) => log.push("untyped " + v));
untyped("u").then((v) => log.push("untyped " + v));
new K().get(1).then((v) => log.push("get " + v));
(K.prototype.get as any).call({ v: 10 }, 1).then((v: any) => log.push("get call " + v));
K.make(2).then((v) => log.push("make " + v));
arrow(1).then((v) => log.push("arrow " + v));
fnExpr(-1).catch((e: any) => log.push("fnExpr " + e.name));
log.push("main end");
Promise.resolve()
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => console.log(log.join("\n")));
