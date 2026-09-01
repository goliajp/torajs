// RFC 20260901-scope-exit-drops — every way out of a scope that skips
// its closing `}` (throw to catch, `throw`, break / continue, labeled
// jumps, return through finally, the finally tail) must release the
// frames it leaves, exactly once, and outer locals must stay alive for
// the code that still reads them (finally bodies, the loop after a
// continue). Output is bun's; the leak itself is the churn probe's job.
const boom = (): any => {
  throw new Error("boom");
};
const s = (n: number): string => "v" + n;
const a = (n: number): number[] => [n, n + 1];
let caught = 0;

// 1. try-body local, normal path
for (let i = 0; i < 200; i++) {
  try {
    const t = s(i);
    if (i === 199) console.log("t1", t);
  } catch (e) {
    caught++;
  }
}
// 2. try-body locals live across a throwing call
for (let i = 0; i < 200; i++) {
  try {
    const t = s(i);
    const u = a(i);
    boom();
    console.log("never", t, u);
  } catch (e) {
    caught++;
  }
}
// 3. `throw` statement with a live local
for (let i = 0; i < 200; i++) {
  try {
    const t = s(i);
    if (i >= 0) throw new Error("x" + t);
  } catch (e) {
    caught++;
  }
}
console.log("caught", caught);

// 4. continue / break past a local
let acc = 0;
for (let i = 0; i < 200; i++) {
  const t = s(i);
  if (i % 2 === 0) continue;
  acc += t.length;
}
for (let i = 0; i < 200; i++) {
  for (let j = 0; j < 3; j++) {
    const t = s(i * j);
    if (j === 1) break;
    acc += t.length;
  }
}
console.log("acc", acc);

// 5. labeled continue out of two frames
let lab = 0;
outer: for (let i = 0; i < 50; i++) {
  const t = s(i);
  for (let j = 0; j < 3; j++) {
    const u = s(j);
    if (j === 2) continue outer;
    lab += t.length + u.length;
  }
}
console.log("lab", lab);

// 6. (switch-case break lives in switch-f64-scrutinee-001.ts — it
//    needs the f64 scrutinee fix that landed with it)

// 7. return through finally; the finally reads an outer local
const rf = (k: number): string => {
  const outer = s(k);
  try {
    const inner = s(k + 1);
    return inner + outer;
  } finally {
    console.log("fin", outer);
  }
};
for (let i = 0; i < 3; i++) console.log(rf(i));

// 8. break routed through finally; finally has its own local and reads
//    the loop body's
let bf = 0;
for (let i = 0; i < 100; i++) {
  const t = s(i);
  try {
    const u = s(i);
    if (i % 3 === 0 && i > 0) break;
    bf += u.length;
  } finally {
    const w = s(i);
    bf += w.length + t.length;
  }
}
console.log("bf", bf);

// 9. rethrow from a catch body via a call — the catch param and the
//    catch-body local leave with it
let rc = 0;
for (let i = 0; i < 100; i++) {
  try {
    try {
      boom();
    } catch (e) {
      const t = s(i);
      boom();
      console.log("never", t);
    }
  } catch (e2) {
    rc++;
  }
}
console.log("rc", rc);

// 10. a closure captures the loop-body local, then break — the capture
//     must outlive the frame the break leaves
let g: () => string = (): string => "none";
let last = "";
for (let i = 0; i < 5; i++) {
  const t = s(i);
  g = (): string => t + "!";
  last = g();
  if (i === 2) break;
}
console.log(g(), last);

// 11. shadowing across a try that throws
let x = "outer";
try {
  let x = "inner";
  if (x.length > 0) boom();
} catch (e) {
  x = x + "!";
}
console.log("x", x);

// 12. for-of continue over fresh strings
let fo = 0;
for (let i = 0; i < 100; i++) {
  for (const w of [s(i), s(i + 1)]) {
    if (w.length > 2) continue;
    fo += w.length;
  }
}
console.log("fo", fo);

// 13. while + break inside a fn
const wf = (k: number): number => {
  let j = 0;
  while (true) {
    const t = s(k + j);
    if (j >= 2) break;
    j += t.length - 1;
  }
  return j;
};
console.log("wf", wf(3));

// 14. do-while continue with a local
let dw = 0;
let di = 0;
do {
  const t = s(di);
  di++;
  if (di % 2 === 0) continue;
  dw += t.length;
} while (di < 20);
console.log("dw", dw);

// 15. return through two finallies, each reading its own outer local
const nf = (k: number): number => {
  const o = a(k);
  try {
    const p = a(k + 1);
    try {
      const q = a(k + 2);
      return o.length + p.length + q.length;
    } finally {
      console.log("f-in", p[0]);
    }
  } finally {
    console.log("f-out", o[0]);
  }
};
console.log("nf", nf(1));
