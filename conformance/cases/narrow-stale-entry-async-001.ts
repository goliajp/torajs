// an async callee whose body reads its parameter's member before an
// await: the TypeError rejects the returned promise, it is not thrown
// at the call
type Node = { n: number };
let q: Node | null = { n: 1 };
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, "sync", (e as Error).constructor.name) }
  q = { n: 1 };
}

async function af(x: Node): Promise<number> {
  const v = x.n;
  await 0;
  return v + 1;
}
run("async", () => {
  if (q !== null && nullQ()) {
    af(q).then((v) => console.log("value", v), (e) => console.log("rejected", (e as Error).constructor.name));
    console.log("after call");
  }
});
af({ n: 4 }).then((v) => console.log("live", v));
