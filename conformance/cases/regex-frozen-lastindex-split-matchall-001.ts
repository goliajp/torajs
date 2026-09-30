// `s.split(re)` (§22.2.6.14) and `s.matchAll(re)` (§22.2.6.9) run on a
// species-constructed copy of the RegExp, so a non-writable lastIndex
// on the original is never stored to: no throw, and the original's
// lastIndex is left alone. Acceptance: byte-equal with bun.

function frozen(flags: string): RegExp {
  const re = new RegExp("a", flags);
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probeSplit(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, s.split(re).join("|"), re.lastIndex);
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

function probeMatchAll(label: string, re: RegExp, s: string): void {
  try {
    let n = 0;
    for (const m of s.matchAll(re)) {
      n = n + m[0].length;
    }
    console.log(label, n, re.lastIndex);
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

probeSplit("split g", frozen("g"), "xax");
probeSplit("split y", frozen("y"), "axa");
probeSplit("split plain", frozen(""), "xax");
probeMatchAll("matchAll g", frozen("g"), "xaxa");
probeMatchAll("matchAll gy", frozen("gy"), "aax");
