// An own property on a RegExp shadows the builtin accessor it is
// named after, for the static member read as well as the computed one.
function t(name: string, f: () => any) {
  try {
    console.log(name, f());
  } catch (e: any) {
    console.log(name, "threw", e.message);
  }
}
t("own global getter", () => {
  const r: any = /./;
  Object.defineProperty(r, "global", { get() { throw new Error("global"); } });
  return r.global;
});
t("own sticky value", () => {
  const r: any = /./;
  Object.defineProperty(r, "sticky", { value: "yes" });
  return r.sticky;
});
t("own flags value", () => {
  const r: any = /a/g;
  Object.defineProperty(r, "flags", { value: "" });
  return JSON.stringify(r.flags);
});
t("own source value", () => {
  const r: any = /a/g;
  Object.defineProperty(r, "source", { value: "zz" });
  return r.source;
});
t("untouched", () => {
  const r: any = /a/gy;
  return [r.global, r.sticky, r.flags, r.source].join(" ");
});
