// `s.match(re)` under `g` / `y` stores lastIndex (§22.2.6.8 step 8.a
// via §22.2.7.2); a lastIndex made non-writable refuses it with a
// TypeError that must surface at the call, inside the caller's try.
// Both consumers of the result are covered: the array itself and the
// null-test / length-only shape. Acceptance: byte-equal with bun.

function frozen(flags: string): RegExp {
  const re = new RegExp("a", flags);
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probe(label: string, re: RegExp, s: string): void {
  try {
    const m = s.match(re);
    console.log(label, m === null ? "null" : m.join("|"));
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

function probeLen(label: string, re: RegExp, s: string): void {
  try {
    const m = s.match(re);
    console.log(label, m === null ? "null" : m[0].length);
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

probe("g hit", frozen("g"), "xaa");
probe("g miss", frozen("g"), "xx");
probe("y hit", frozen("y"), "a");
probe("y miss", frozen("y"), "xa");
probe("plain", frozen(""), "xa");
probe("control g", /a/g, "xaa");
probe("control y", /a/y, "a");
probeLen("len g hit", frozen("g"), "xa");
probeLen("len y hit", frozen("y"), "a");
probeLen("len plain", frozen(""), "xa");
probeLen("len control g", /a/g, "xa");
