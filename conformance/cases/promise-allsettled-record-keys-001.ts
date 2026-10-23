// The record a tag-less allSettled site builds is an ordinary object
// with exactly two own, enumerable, writable data properties, in the
// order §27.2.4.3.1 creates them: `status` first, then `value` or
// `reason`. The dyn entry (an `any` iterable) and the fan-in over a
// pending element both build it.
async function later(v: any): Promise<any> {
  await null;
  return v;
}
function inputs(): any {
  return [1, Promise.reject("no"), later({ k: "v" }), later("s")];
}
async function main() {
  const rs: any = await Promise.allSettled(inputs());
  for (const r of rs) {
    console.log(Object.keys(r).join(","), JSON.stringify(r));
  }
  const r0: any = rs[0];
  r0.value = 5;
  r0.extra = true;
  console.log(JSON.stringify(r0), Object.keys(r0).length);
}
main();
