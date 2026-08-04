// allSettled's settled records are real objects, and the rejected
// ones say `reason`.
//
// The records were 48 anonymous bytes with class_tag 0, which no
// by-name lookup can find: an unannotated handler (parameter inferred
// `any`) read them as {} and JSON.stringify agreed. An ANNOTATED
// handler was always fine, because the checker types the element
// Struct([status, value]) and the static read lands on the fixed
// offsets — which is what kept this out of sight.
//
// Case D is the field name §27.2.4.2 gives each outcome: `value` when
// fulfilled, `reason` when rejected. One layout cannot answer to two
// names at the same offset, so the two shapes carry separate tags.
// This was invisible until the records became readable at all.

async function main() {
  // A — JSON, the shape a reader sees whole.
  const a: any = await Promise.allSettled([Promise.resolve(1), Promise.reject(2)]);
  console.log("A", JSON.stringify(a));

  // B — direct field reads through the any lane.
  console.log("B", a[0].status, a[0].value);

  // C — heap values in the record.
  const c: any = await Promise.allSettled([Promise.resolve("s1"), Promise.resolve("s2")]);
  console.log("C", JSON.stringify(c));

  // D — the rejected record answers to `reason`, not `value`.
  console.log("D", a[1].status, a[1].reason, a[1].value);

  // E — a mixed (Array<Any>) input goes through the any-lane sibling,
  // which builds the same records.
  const e: any = await Promise.allSettled([Promise.resolve(1), 2]);
  console.log("E", JSON.stringify(e));

  // F — an annotated handler still reads the static offsets, the way
  // it did before the records had any identity at all.
  Promise.allSettled([Promise.resolve(7)]).then((recs: Rec[]) => {
    console.log("F", recs[0].status, recs[0].value);
  });
}

type Rec = { status: string; value: number };

main();
