for (const v of [1, null, {}, undefined, "s"]) {
  try { new Promise(v as any); console.log("no"); } catch (e) { console.log(e instanceof TypeError, (e as any).message); }
}
try { new (Promise as any)(); console.log("no"); } catch (e) { console.log("0-arg", e instanceof TypeError); }
let order: string[] = [];
const p = new Promise((res) => { order.push("exec"); res(1); }, (order.push("extra"), 2) as any);
p.then((v) => console.log(order.join(","), v));
const q = new Promise((_, rej) => { throw new Error("boom"); });
q.catch((e) => console.log("rej", (e as Error).message));
