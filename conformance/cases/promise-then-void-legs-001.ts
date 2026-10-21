// A two-handler `.then` whose legs return nothing settles undefined,
// whichever leg runs and however the legs declare their parameter.

async function main(): Promise<void> {
  const ok: Promise<number> = Promise.resolve(1);
  const a = await ok.then((v: any) => {
    console.log("ok leg", v);
  }, (e: any) => {
    console.log("err leg", e.message);
  });
  console.log("any legs fulfilled", a, a === undefined);
  const bad: Promise<number> = Promise.reject(new Error("no"));
  const b = await bad.then((v: any) => {
    console.log("ok leg", v);
  }, (e: any) => {
    console.log("err leg", e.message);
  });
  console.log("any legs rejected", b, b === undefined);
  const c = await ok.then((v: number) => {
    console.log("typed ok leg", v + 1);
  }, (e: any) => {
    console.log("err leg", e.message);
  });
  console.log("mixed legs", c, c === undefined);
  const d = await bad.then((v: any) => {
    console.log("ok leg", v);
  }, (e: any) => undefined);
  console.log("void and undefined", d, d === undefined);
}
main();
