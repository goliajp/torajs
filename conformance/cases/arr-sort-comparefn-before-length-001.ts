// §23.1.3.30 step 1: a comparefn that is neither undefined nor callable
// throws a TypeError before ToObject / LengthOfArrayLike, so a
// receiver's length getter never runs.
const poisoned = {
  get length() {
    throw new Error("length read");
  },
};
for (const c of [null, 1, "s", true, {}, [], Symbol("q")] as any[]) {
  try {
    ([] as any[]).sort.call(poisoned, c);
    console.log("no throw");
  } catch (e) {
    console.log(c === null ? "null" : typeof c, (e as Error).constructor.name);
  }
}
try {
  ([] as any[]).sort.call(poisoned, undefined);
} catch (e) {
  console.log("undefined", (e as Error).message);
}
