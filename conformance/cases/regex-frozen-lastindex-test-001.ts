// `re.test(s)` under `g` / `y` stores lastIndex through
// Set(R, "lastIndex", …, true) (§22.2.7.2 step 12.a / 15); a lastIndex
// made non-writable refuses it with a TypeError that must surface at
// the call, inside the caller's try. Acceptance: byte-equal with bun.

function frozen(flags: string): RegExp {
  const re = new RegExp("a", flags);
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probe(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, re.test(s));
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError);
  }
}

probe("g hit", frozen("g"), "xa");
probe("g miss", frozen("g"), "xx");
probe("y hit", frozen("y"), "a");
probe("y miss", frozen("y"), "xa");
probe("plain", frozen(""), "xa");
probe("control g", /a/g, "xa");
probe("control y", /a/y, "a");
