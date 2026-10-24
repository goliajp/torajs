// `arguments` and `new.target` in async functions that suspend: the
// arguments object is made from the call's arguments on entry
// (§10.2.11 FunctionDeclarationInstantiation step 22, before the body
// runs) and is the same object after every await, an arrow reads its
// enclosing function's, and an async function is never a constructor
// (§15.8, no [[Construct]]), so `new.target` is undefined, also in an
// arrow inside it.
const log: string[] = [];
async function a(x: number, y?: number): Promise<number> {
  const n = arguments.length;
  await null;
  return n * 100 + arguments.length * 10 + x;
}
async function b(...r: number[]): Promise<string> {
  await 0;
  return "b " + arguments[1] + " " + arguments.length + " " + r.length;
}
async function c(): Promise<boolean> {
  await 1;
  const f = () => new.target;
  return new.target === undefined && f() === undefined;
}
async function d(): Promise<number> {
  const f = () => arguments.length;
  await null;
  return f();
}
async function e(p: any, q: any) {
  await null;
  arguments[0] = "changed";
  return [p, arguments[0], arguments.length].join("/");
}
async function esc() {
  const args = arguments;
  await null;
  return args;
}
async function sum() {
  let s = 0;
  for (let i = 0; i < arguments.length; i++) {
    await null;
    s += arguments[i];
  }
  return s;
}
async function rejects() {
  await null;
  throw new Error("args " + arguments.length);
}
class K {
  async m(k: number) {
    await null;
    return arguments.length + k;
  }
  static async s() {
    await null;
    return arguments.length;
  }
}
const fa = async function () {
  await null;
  return arguments.length;
};
async function main() {
  log.push("a " + (await a(1)));
  log.push("a2 " + (await a(1, 2)));
  log.push("a3 " + (await (a as any)(1, 2, 3)));
  log.push(await b(4, 5, 6));
  log.push("c " + (await c()));
  log.push("d " + (await (d as any)(1, 2, 3)));
  log.push("e " + (await e("p", "q")));
  const r: any = await (esc as any)(7, 8);
  log.push("esc " + r.length + " " + r[1]);
  log.push("sum " + (await (sum as any)(1, 2, 3, 4)));
  try {
    await (rejects as any)(1, 2);
  } catch (err: any) {
    log.push("rejects " + err.message);
  }
  log.push("m " + (await (new K() as any).m(1, 2)));
  log.push("s " + (await (K as any).s(1, 2, 3)));
  log.push("fa " + (await (fa as any)(9, 8)));
  console.log(log.join("\n"));
}
main();
