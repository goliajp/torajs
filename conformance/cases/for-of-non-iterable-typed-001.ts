// GetIterator on a non-iterable is a TypeError (§7.4.3), and the any
// lane raises exactly that. The typed spelling used to be a COMPILE
// refusal: a specialized body's `for (const q of x)` with x bound to 1
// said "for-of source type I64 not yet supported", while the same loop
// over `const n: any = 1` threw. Every shape the compiler can name now
// boxes into the any lane instead. Strings ARE iterable and keep
// their own lane; that is checked too.
function t(tag: string, f: () => unknown) {
  try {
    const r = f();
    console.log(tag, "NO-THROW ->", JSON.stringify(r));
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function count(x) {
  let n = 0;
  for (const q of x) {
    n = n + (q === undefined ? 1 : 1);
  }
  return n;
}
t("number", () => count(1));
t("boolean", () => count(true));
t("float", () => count(1.5));
t("string", () => count("abc"));
t("date", () => count(new Date(0)));
t("regexp", () => count(/x/));
t("function", () => count(() => 1));
t("bigint", () => count(10n));
t("array", () => count([1, 2, 3]));
t("map", () => count(new Map([[1, 2]])));
