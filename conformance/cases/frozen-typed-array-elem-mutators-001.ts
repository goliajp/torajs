// sort / reverse / fill / copyWithin write EXISTING slots, so the rule
// is §10.1.9's [[Writable]], not [[Extensible]]: only a FROZEN
// receiver refuses — a sealed or preventExtensions one keeps writable
// elements and must still mutate (read off node and bun; push /
// unshift's predicate would have been wrong here). Four of the nine
// mutators wrote straight through a freeze: Object.freeze([3,1,2])
// .sort() answered [1,2,3].
//
// fill on a refcounted element type, sort's insertion path and
// copyWithin on refcounted elements all write slots INLINE with no
// kernel call, so the guard is emitted at each method's entry after
// its arguments are evaluated; reverse goes through the kernel and
// takes its check after the call.
function t(tag: string, f: () => unknown) {
  try {
    const r = f();
    console.log(tag, "NO-THROW ->", JSON.stringify(r));
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
function mk(): number[] {
  return [3, 1, 2];
}
function ms(): string[] {
  return ["c", "a", "b"];
}
for (const mode of ["freeze", "seal", "ne", "plain"]) {
  const lock = (a: any) => {
    if (mode === "freeze") Object.freeze(a);
    else if (mode === "seal") Object.seal(a);
    else if (mode === "ne") Object.preventExtensions(a);
    return a;
  };
  t(mode + " num sort", () => lock(mk()).sort());
  t(mode + " num sort-cb", () => lock(mk()).sort((x: number, y: number) => y - x));
  t(mode + " num reverse", () => lock(mk()).reverse());
  t(mode + " num fill", () => lock(mk()).fill(0));
  t(mode + " num fill-range", () => lock(mk()).fill(9, 1, 2));
  t(mode + " num copyWithin", () => lock(mk()).copyWithin(0, 1));
  t(mode + " str sort", () => lock(ms()).sort());
  t(mode + " str reverse", () => lock(ms()).reverse());
  t(mode + " str fill", () => lock(ms()).fill("z"));
  t(mode + " str copyWithin", () => lock(ms()).copyWithin(0, 1));
}
// the receiver is untouched after a refusal
const n: number[] = [3, 1, 2];
Object.freeze(n);
try { n.sort(); } catch (e) { /* refused */ }
try { n.fill(7); } catch (e) { /* refused */ }
console.log("frozen num after refusals ->", JSON.stringify(n));
// argument evaluation precedes the refusal (§23.1.3.7 step order)
let evaluated = 0;
function val(): number {
  evaluated = evaluated + 1;
  return 5;
}
try { n.fill(val()); } catch (e) { /* refused */ }
console.log("fill arg evaluated", evaluated, "time(s)");
// §10.1.9 raises at the Set, so a frozen receiver that nothing would be
// written to is legal — test262 freezes an EMPTY array before fill and
// a ONE-entry array before reverse. The first version of the guard
// raised at entry and failed both; each method now tests its own
// "a slot will be written" predicate first (read off node and bun):
// fill a non-empty range, reverse/sort two or more elements,
// copyWithin a positive count.
function nw(tag: string, mk: () => any, f: (a: any) => void) {
  const a = mk();
  Object.freeze(a);
  try { f(a); console.log("nowrite", tag, "NO-THROW", JSON.stringify(a)); }
  catch (e) { console.log("nowrite", tag, "threw", (e as Error).constructor.name); }
}
nw("fill []", () => [] as number[], (a: any) => a.fill(1));
nw("fill [1] empty range", () => [1] as number[], (a: any) => a.fill(9, 1, 1));
nw("fill [1] full", () => [1] as number[], (a: any) => a.fill(9));
nw("reverse [1]", () => [1] as number[], (a: any) => a.reverse());
nw("reverse []", () => [] as number[], (a: any) => a.reverse());
nw("sort []", () => [] as number[], (a: any) => a.sort());
nw("sort [1]", () => [1] as number[], (a: any) => a.sort());
nw("sort [2,1]", () => [2, 1] as number[], (a: any) => a.sort());
nw("copyWithin [1] count0", () => [1] as number[], (a: any) => a.copyWithin(0, 1));
nw("copyWithin [1,2] same", () => [1, 2] as number[], (a: any) => a.copyWithin(0, 0));
nw("copyWithin []", () => [] as number[], (a: any) => a.copyWithin(0, 0));
