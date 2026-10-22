// Promise.all over a typed array whose elements carry own properties
// other than `then`, in a program that never writes `then`.
//
// The expando bag is there, so the own-`then` question has to be
// asked of it rather than answered by the bag's absence; it answers
// no, and the input keeps the plain walk.

async function main() {
  const arr = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)];
  Object.defineProperty(arr[0], "label", { value: "first" });
  (arr[2] as any).note = 7;
  const r = await Promise.all(arr);
  console.log(r[0] + r[1] + r[2], r.length);
  console.log((arr[0] as any).label, (arr[2] as any).note);

  const empty: Promise<number>[] = [];
  const e = await Promise.all(empty);
  console.log("empty", e.length);
}

main();
