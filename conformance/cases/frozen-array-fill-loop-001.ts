// The pre-reserved fill loop stores slots inline — no kernel call per
// push — so a frozen receiver used to be filled straight through the
// freeze: the one kernel call, `arr_reserve` above the loop, returns
// early when capacity already suffices. The lane now hoists the
// receiver's [[Extensible]] bits and the trip count into the preheader:
// a locked receiver with at least one trip raises before the loop, and
// a zero-trip loop never throws (b) — JS never touches the array then.
// (A per-append check inside the loop was measured at +57..69% on
// array-sum-1m and rejected; the one thing this trades — an impure push
// argument is not evaluated before the throw — is registered, not hidden.)
function t(tag: string, f: () => unknown) {
  try {
    const r = f();
    console.log(tag, "NO-THROW ->", r);
  } catch (e) {
    console.log(tag, "threw", (e as Error).constructor.name);
  }
}
// canonical for-fill over a frozen number[]
t("for-fill frozen", () => {
  const xs: number[] = [7];
  Object.freeze(xs);
  for (let i = 0; i < 5; i = i + 1) {
    xs.push(i);
  }
  return xs.length;
});
// (b) zero-trip loop: no push, no throw
t("for-fill zero-trip", () => {
  const xs: number[] = [7];
  Object.freeze(xs);
  const n = 0;
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i);
  }
  return xs.length;
});
// while-fill spelling
t("while-fill frozen", () => {
  const xs: number[] = [];
  Object.freeze(xs);
  let i = 0;
  while (i < 3) {
    xs.push(i);
    i = i + 1;
  }
  return xs.length;
});
// sealed / preventExtensions receivers refuse the same way
t("for-fill sealed", () => {
  const xs: number[] = [1];
  Object.seal(xs);
  for (let i = 0; i < 2; i = i + 1) {
    xs.push(i);
  }
  return xs.length;
});
t("for-fill preventExtensions", () => {
  const xs: string[] = ["a"];
  Object.preventExtensions(xs);
  for (let i = 0; i < 2; i = i + 1) {
    xs.push("b");
  }
  return xs.length;
});
// an extensible receiver still fills, and the fast path still
// answers the new length
t("for-fill extensible", () => {
  const xs: number[] = [];
  let last = 0;
  for (let i = 0; i < 4; i = i + 1) {
    last = xs.push(i * i);
  }
  return last + ":" + JSON.stringify(xs);
});
// an impure push argument: JS evaluates it once before the first
// push raises (§23.1.3.23 step 5 runs before Set's TypeError), so a
// frozen receiver still sees exactly one evaluation, and an extensible
// one sees every evaluation
let evaluated = 0;
function arg(i: number): number { evaluated = evaluated + 1; return i; }
t("for-fill frozen impure arg", () => {
  const xs: number[] = [7];
  Object.freeze(xs);
  evaluated = 0;
  try { for (let i = 0; i < 5; i = i + 1) { xs.push(arg(i)); } } catch (e) { return "threw " + (e as Error).constructor.name + " after " + evaluated + " evaluation(s)"; }
  return "no throw, " + evaluated;
});
t("for-fill extensible impure arg", () => {
  const xs: number[] = [7];
  evaluated = 0;
  for (let i = 0; i < 5; i = i + 1) { xs.push(arg(i)); }
  return evaluated + ":" + JSON.stringify(xs);
});
t("while-fill sealed impure arg", () => {
  const xs: number[] = [];
  Object.seal(xs);
  evaluated = 0;
  let i = 0;
  try { while (i < 3) { xs.push(arg(i)); i = i + 1; } } catch (e) { return "threw " + (e as Error).constructor.name + " after " + evaluated + " evaluation(s)"; }
  return "no throw, " + evaluated;
});
