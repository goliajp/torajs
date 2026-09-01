// A `switch` clause value compares at the scrutinee's width. `i % 2`
// under the f64 width class against a literal `0` used to hand `FCmp`
// an integer constant the FPR materializer cannot hold — the build
// aborted with "FPR materialization can't hold ConstI64(0)" as soon as
// another loop in the program had a `break` (the width class is a
// whole-program verdict). The mirror — an integer scrutinee against a
// fractional clause — compares numerically instead of never matching.
const boom = (): any => {
  throw new Error("boom");
};
const s = (n: number): string => "v" + n;
const n = (k: number): number => k;

// the original shape: a `break` elsewhere + a `switch (i % 2)` in a try
let acc = 0;
for (let j = 0; j < 3; j++) {
  if (j === 1) break;
  acc += j;
}
let sw = 0;
for (let i = 0; i < 4; i++) {
  try {
    switch (i % 2) {
      case 0: {
        sw += 1;
        break;
      }
      case 1:
        boom();
    }
  } catch (e) {
    sw += 100;
  }
}
console.log(acc, sw);

// integer scrutinee, fractional clause value
let hit = 0;
for (let i = 0; i < 4; i++) {
  switch (n(i)) {
    case 1.5:
      hit += 100;
      break;
    case 2:
      hit += 1;
      break;
    default:
      hit += 10;
  }
}
console.log(hit);

// scope-exit-drops: a case-block `break` past an owned local, and a
// clause that jumps past a `const` declared in an earlier clause —
// twice, so the slot is re-entered
let sw2 = 0;
for (let i = 0; i < 200; i++) {
  switch (i % 2) {
    case 0: {
      const u = s(i);
      sw2 += u.length;
      break;
    }
    default:
      sw2++;
  }
}
for (let i = 0; i < 4; i++) {
  try {
    switch (i % 2) {
      case 0: {
        const t = s(i);
        sw2 += t.length;
        break;
      }
      case 1:
        boom();
    }
  } catch (e) {
    sw2 += 100;
  }
}
console.log("sw", sw2);
