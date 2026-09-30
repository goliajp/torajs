// `s.search(re)` first resets a lastIndex that is not SameValue +0
// (§22.2.6.12 step 4); a non-writable lastIndex refuses that store with
// a TypeError even without `g` / `y`. At +0 nothing is stored and the
// search answers. Acceptance: byte-equal with bun.

function frozenAt(last: number, flags: string): RegExp {
  const re = new RegExp("a", flags);
  re.lastIndex = last;
  Object.defineProperty(re, "lastIndex", { writable: false });
  return re;
}

function probe(label: string, re: RegExp, s: string): void {
  try {
    console.log(label, s.search(re), re.lastIndex);
  } catch (e) {
    console.log(label, "caught", e instanceof TypeError, re.lastIndex);
  }
}

probe("plain at 0", frozenAt(0, ""), "xa");
probe("plain at 3", frozenAt(3, ""), "xa");
probe("plain at 3 miss", frozenAt(3, ""), "xx");
probe("plain at -0", frozenAt(-0, ""), "xa");
probe("i at 1", frozenAt(1, "i"), "xA");
probe("g at 0", frozenAt(0, "g"), "xa");

const live = /a/;
live.lastIndex = 5;
probe("writable at 5", live, "xa");
const sealed = /a/;
sealed.lastIndex = 2;
Object.freeze(sealed);
probe("frozen object at 2", sealed, "xa");
