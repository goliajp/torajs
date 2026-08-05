// `Promise.all`'s result array held one raw form chosen from the
// static element type, and a raw slot cannot carry the difference
// between "null" and "the value": a Str-formed array read a NULL slot
// back as undefined, a number-formed one read it as `0`, and an
// all-null input named no form at all and threw at attach.
//
// Naming the tagged lane hands the question to the runtime, which
// boxes each element off that promise's own stamp — where null and
// undefined were never conflated to begin with.
const s: string | null = null;
Promise.all([Promise.resolve(s), Promise.resolve("x")]).then((r) => {
  console.log("mixed str:", r[0], r[1]);
});

const n: number | null = null;
Promise.all([Promise.resolve(n), Promise.resolve(1)]).then((r) => {
  console.log("mixed num:", r[0], r[1]);
});

// A whole array of them, which used to name no lane at all.
Promise.all([Promise.resolve(null), Promise.resolve(null)]).then((r) => {
  console.log("all null:", r[0], r[1]);
});

// undefined is the other half the collapsed static type folded
// together with null — each element's own stamp keeps them apart.
Promise.all([Promise.resolve(undefined)]).then((r) => {
  console.log("all undefined:", r[0]);
});

// A nullable holding an actual value still reads as that value.
const v: string | null = "hi";
Promise.all([Promise.resolve(v), Promise.resolve("x")]).then((r) => {
  console.log("non-null:", r[0], r[1]);
});

// The ordinary homogeneous shapes keep their raw lane and their answers.
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then((r) => {
  console.log("nums:", r[0], r[1], r.length);
});
Promise.all([Promise.resolve("a"), Promise.resolve("b")]).then((r) => {
  console.log("strs:", r[0] + r[1]);
});
