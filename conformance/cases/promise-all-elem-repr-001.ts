// Promise.all builds its result array in the element form the call
// site names.
//
// An executor-minted cell settles through the any lane, so its slot
// honestly holds a NaN box and its stamp honestly says REPR_ANY. The
// result array had no shape that fits that: it was left undescribed,
// which the any-param handler refused loudly (case A) while a static
// read of the same array bitcast the box bits and answered NaN
// silently (case B). One cause, two faces — the checker knows the
// call is Promise<T[]>, so the site hands that element form down and
// the kernel unboxes each element into it.
//
// Cases C and D are the regression guard: elements minted through
// Promise.resolve were already in raw form and must stay untouched.

function mkNum(v: number): Promise<number> {
  return new Promise((res) => {
    res(v);
  });
}

function mkStr(v: string): Promise<string> {
  return new Promise((res) => {
    res(v + "-tail");
  });
}

async function main() {
  // A — the loud face: an any-param handler over executor-minted cells.
  Promise.all([mkNum(1), mkNum(2)]).then((a) => console.log("A", a[0], a[1]));

  // B — the silent face: a static read has to see numbers, not boxes.
  const b = await Promise.all([mkNum(10), mkNum(20)]);
  console.log("B", b[0] + b[1]);

  // C — heap elements keep working, and the result array owns them
  // past the death of the promises it read them out of.
  const kept: string[] = [];
  for (let i = 0; i < 3; i++) {
    const pair = await Promise.all([mkStr("x" + i), mkStr("y" + i)]);
    kept.push(pair[0]);
    kept.push(pair[1]);
  }
  console.log("C", kept.join(","));

  // D — the already-working shape stays byte-identical.
  const d = await Promise.all([Promise.resolve(7), Promise.resolve(8)]);
  console.log("D", d[0] + d[1]);
}

main();
