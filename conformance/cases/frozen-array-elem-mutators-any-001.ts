// sort / reverse / fill / copyWithin write EXISTING slots, so the rule
// is §10.1.9's [[Writable]], not [[Extensible]]: only a FROZEN receiver
// refuses — a sealed or preventExtensions one keeps writable elements
// and must still mutate. Read off node; push/unshift's predicate would
// be wrong here.
function t(tag: string, mode: string, f: (a: any) => void) {
  const a: any[] = [3, 1, 2];
  if (mode === "freeze") Object.freeze(a);
  else if (mode === "seal") Object.seal(a);
  else if (mode === "ne") Object.preventExtensions(a);
  try {
    f(a);
    console.log(mode, tag, "NO-THROW ->", JSON.stringify(a));
  } catch (e) {
    console.log(mode, tag, "threw", (e as Error).constructor.name, JSON.stringify(a));
  }
}
for (const mode of ["freeze", "seal", "ne", "plain"]) {
  t("sort", mode, (a: any) => a.sort());
  t("sort-cb", mode, (a: any) => a.sort((x: number, y: number) => y - x));
  t("reverse", mode, (a: any) => a.reverse());
  t("fill", mode, (a: any) => a.fill(0));
  t("copyWithin", mode, (a: any) => a.copyWithin(0, 1));
}
// typed receivers take the typed kernels
const n: number[] = [3, 1, 2];
Object.freeze(n);
for (const [tag, f] of [
  ["num sort", () => n.sort()],
  ["num reverse", () => n.reverse()],
  ["num fill", () => n.fill(0)],
  ["num copyWithin", () => n.copyWithin(0, 1)],
] as [string, () => unknown][]) {
  try { f(); console.log(tag, "NO-THROW ->", JSON.stringify(n)); }
  catch (e) { console.log(tag, "threw", (e as Error).constructor.name, JSON.stringify(n)); }
}
// no-write calls on a frozen any[] are legal (see the typed fixture)
function nw(tag: string, mk: () => any[], f: (a: any) => void) {
  const a = mk();
  Object.freeze(a);
  try { f(a); console.log("nowrite", tag, "NO-THROW", JSON.stringify(a)); }
  catch (e) { console.log("nowrite", tag, "threw", (e as Error).constructor.name); }
}
nw("fill []", () => [], (a: any) => a.fill(1));
nw("reverse [1]", () => [1], (a: any) => a.reverse());
nw("sort [1]", () => [1], (a: any) => a.sort());
nw("copyWithin [1] count0", () => [1], (a: any) => a.copyWithin(0, 1));
nw("sort [2,1]", () => [2, 1], (a: any) => a.sort());
