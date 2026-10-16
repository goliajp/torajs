// `new R(pattern, flags)` through a first-class RegExp constructor
// value: a RegExp pattern contributes its source (not its `/src/f`
// text), IsRegExp runs first, and a regexp-like object answers
// `source` / `flags` by Get.
const R: any = RegExp;
function t(name: string, f: () => any) {
  try {
    console.log(name, f());
  } catch (e: any) {
    console.log(name, "threw", e.constructor.name);
  }
}
t("regexp pattern, new flags", () => {
  const s: any = new R(/b/, "y");
  s.lastIndex = 1;
  return [s.source, s.flags, JSON.stringify(s.exec("abc")), s.lastIndex].join(" ");
});
t("regexp pattern, own flags", () => String(new R(/a+/gi)));
t("IsRegExp getter runs first", () => {
  const re: any = /a/;
  Object.defineProperty(re, Symbol.match, {
    get() {
      re.compile("b");
      return true;
    },
  });
  return String(new R(re, "g"));
});
t("regexp-like object", () => {
  const like = { [Symbol.match]: true, source: "x.z", flags: "i" };
  return String(new R(like));
});
t("regexp-like source getter throws", () => {
  const like = {
    [Symbol.match]: true,
    get source(): string {
      throw new Error("source");
    },
  };
  return String(new R(like));
});
// @@split mints its splitter as `new C(rx, flags + "y")`
t("split through the default species", () => {
  const r: any = /b/;
  r.constructor = undefined;
  return JSON.stringify(RegExp.prototype[Symbol.split].call(r, "abcbd"));
});
t("split limit recompiles the receiver", () => {
  const r = /a/;
  const limit = { valueOf() { r.compile("b"); return -1; } };
  return JSON.stringify((r as any)[Symbol.split]("abba", limit));
});
