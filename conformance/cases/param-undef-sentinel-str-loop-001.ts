// A string parameter handed an out-of-range read is checked before
// each member read off it. The check of one value answers the same
// every time, so a scanner loop reading `.length` and `charCodeAt` per
// character needs it once; a missing string must still raise the
// TypeError at its first read, before the loop body runs, and a
// binding assigned a new string inside the loop is checked again.
const ss: string[] = ["a1 b22", "xyz"];

function scan(src: string): number {
  let i = 0;
  let sum = 0;
  while (i < src.length) {
    const c = src.charCodeAt(i);
    if (c >= 48 && c <= 57) {
      let v = 0;
      while (i < src.length) {
        const d = src.charCodeAt(i);
        if (d < 48 || d > 57) break;
        v = v * 10 + (d - 48);
        i = i + 1;
      }
      sum = sum + v;
    } else {
      i = i + 1;
    }
  }
  return sum;
}

function scanLogged(src: string, log: string[]): number {
  let n = 0;
  log.push("before");
  while (n < src.length) {
    log.push("body");
    n = n + 1;
  }
  return n;
}

function rebound(src: string, other: string[]): number {
  let n = 0;
  let cur = src;
  for (let k = 0; k < 3; k = k + 1) {
    n = n + cur.length;
    cur = other[k + 1];
  }
  return n;
}

function line(tag: string, f: () => unknown) {
  try {
    console.log(tag, f());
  } catch (e) {
    console.log(tag, "THREW", (e as Error).name);
  }
}

line("live", () => scan(ss[0]));
line("live-2", () => scan(ss[1]));
line("missing", () => scan(ss[7]));
const log: string[] = [];
line("missing-logged", () => scanLogged(ss[7], log));
console.log(log.join(","));
const log2: string[] = [];
line("live-logged", () => scanLogged(ss[1], log2));
console.log(log2.join(","));
line("rebound", () => rebound(ss[0], ss));
line("rebound-live", () => rebound(ss[0], ["p", "qq", "rrr", "ssss"]));
