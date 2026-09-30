// A string parameter that arrived undefined is checked again after any
// join where only one path saw it checked: a read after a caught throw
// must throw again, a read guarded on one branch must not cover the
// read past the merge, and a check inside a loop body must not cover a
// read that follows the loop. Acceptance: byte-equal with bun.
const ss: string[] = ["a1 b22", "xyz"];

function afterCatch(s: string): number {
  try {
    void s.length;
  } catch (e) {
    console.log("first", (e as Error).name);
  }
  return s.charCodeAt(0);
}

function oneArm(s: string, flag: boolean): number {
  let n = 0;
  if (flag) n = s.length;
  return n + s.charCodeAt(0);
}

function afterLoop(s: string, rounds: number): number {
  let n = 0;
  for (let k = 0; k < rounds; k = k + 1) n = n + s.length;
  return n + s.charCodeAt(0);
}

function line(tag: string, f: () => unknown) {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "THREW", (e as Error).name);
  }
}

line("after-catch", () => afterCatch(ss[7]));
line("after-catch-live", () => afterCatch(ss[1]));
line("one-arm", () => oneArm(ss[7], false));
line("one-arm-taken", () => oneArm(ss[7], true));
line("one-arm-live", () => oneArm(ss[0], true));
line("after-loop-zero", () => afterLoop(ss[7], 0));
line("after-loop", () => afterLoop(ss[7], 2));
line("after-loop-live", () => afterLoop(ss[1], 2));
