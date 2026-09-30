// `s.search(re)` runs RegExpExec (§22.2.6.12 step 6), whose `g` / `y`
// lastIndex store a non-writable lastIndex refuses with a TypeError;
// it must surface at the call, inside the caller's try. Without `g` /
// `y` nothing is stored and the search answers. Acceptance: byte-equal
// with bun.

function frozen(flags: string): RegExp {
  const re = new RegExp("a", flags);
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probe(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, s.search(re));
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
probe("control y", /a/y, "xa");
