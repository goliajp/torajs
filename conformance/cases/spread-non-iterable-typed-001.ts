// `[...x]` on a non-iterable is a TypeError (§7.4.3). The typed
// spelling with x bound to a number fell through to the array path
// and read the integer as an array pointer — SIGSEGV — while
// `const n: any = 1; [...n]` threw. Every shape with no iteration of
// its own now materializes through the any lane, which raises.
function t(tag: string, f: () => unknown) {
  try {
    const r = f();
    console.log(tag, "NO-THROW ->", JSON.stringify(r));
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function spread(x) {
  return [...x];
}
t("number", () => spread(1));
t("boolean", () => spread(false));
t("float", () => spread(2.5));
t("string", () => spread("ab"));
t("date", () => spread(new Date(0)));
t("regexp", () => spread(/x/));
t("function", () => spread(() => 1));
t("bigint", () => spread(7n));
t("array", () => spread([1, 2]));
t("set", () => spread(new Set([3])));
