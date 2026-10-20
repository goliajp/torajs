// The promise `.then(onOk, onErr)` derives settles from whichever
// handler runs, so its value is either handler's return.
function onF(v: number): number {
  return v + 1;
}
function onR(e: string): string {
  return "rejected " + e;
}
function onVoid(e: string): void {
  console.log("void leg", e);
}
const q = Promise.reject("e").then(onF, onR);
q.then((v) => console.log("named", v));
const s = Promise.reject<number>("e");
s.then(onF, onR).then((v) => console.log("typed source", v));
const a = Promise.reject("e").then((v: number) => v + 1, (e: string) => "arrow " + e);
a.then((v: any) => console.log("arrows", v));
const ok = Promise.resolve(1).then((v: number) => v + 1, (e: any) => "never " + e);
ok.then((v: any) => console.log("fulfilled leg", v));
const nv = Promise.reject("x").then((v: number) => v * 2, onVoid);
nv.then((v: any) => console.log("void result", v));
async function main() {
  const v = await Promise.reject("e").then(onF, onR);
  console.log("awaited", v);
  const w = await Promise.resolve(41).then(onF, (e: any) => "never " + e);
  console.log("awaited ok", w);
}
main();
