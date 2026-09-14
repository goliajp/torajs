// A label on a for-of whose source is not a bare identifier: the
// parser hoists such a source into a let ahead of the loop, and the
// label used to wrap that block instead of the loop — `continue
// outer` was refused as "does not label a loop", and inside a
// generator the state machine never saw the labeled loop. The label
// now sits on the loop itself, so both jumps resolve, at top level
// and inside a machine.
const log: string[] = [];
outer: for (const a of [1, 2]) {
  for (const b of [10, 20]) {
    log.push(a + ":" + b);
    if (b === 20) continue outer;
  }
  log.push("unreached");
}
outer2: for (const a of [1, 2]) {
  for (const b of [10, 20]) { if (b === 10) break outer2; log.push("no"); }
}
console.log("top", log.join(","));

function* gen(): Generator<number> {
  const out: number[] = [];
  outer: for (const a of [1, 2, 3]) {
    for (const b of [1, 2]) {
      yield a * 10 + b;
      if (b === 2) continue outer;
      if (a === 3) break outer;
    }
  }
}
console.log("gen", [...gen()].join(","));

async function af(): Promise<string> {
  const seen: string[] = [];
  outer: for (const a of [1, 2]) {
    for (const b of [1, 2]) {
      await null;
      seen.push(a + "." + b);
      if (b === 1) continue outer;
    }
  }
  return seen.join(",");
}
af().then((v: any) => console.log("af", v));
