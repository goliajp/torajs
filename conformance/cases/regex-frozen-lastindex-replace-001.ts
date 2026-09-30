// `s.replace(re, str)` / `s.replaceAll(re, str)` store lastIndex under
// `g` (§22.2.6.11 step 10) and under `y` (the one exec's §22.2.7.2
// store); a lastIndex made non-writable refuses either with a
// TypeError that must surface at the call, inside the caller's try.
// Acceptance: byte-equal with bun.

function frozen(flags: string): RegExp {
  const re = new RegExp("a", flags);
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probe(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, s.replace(re, "b"));
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

function probeAll(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, s.replaceAll(re, "b"));
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

probe("g hit", frozen("g"), "xaa");
probe("g miss", frozen("g"), "xx");
probe("y hit", frozen("y"), "aa");
probe("y miss", frozen("y"), "xa");
probe("plain", frozen(""), "xaa");
probe("control g", /a/g, "xaa");
probe("control y", /a/y, "aa");
probeAll("all g hit", frozen("g"), "xaa");
probeAll("all g miss", frozen("g"), "xx");
probeAll("all control g", /a/g, "xaa");
