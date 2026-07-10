// chunk 752 — Copy-result return/throw must not strand the member
// receiver's scope drop: `return v.length` (typed str / any / alias)
// and `throw v.length` previously marked v moved and leaked its cell.
// Values must match bun; the leak itself is guarded by AOT RSS probes.
function f1(): number {
  const v = "xy" + "z!";
  return v.length;
}
function f2(): number {
  const v: any = "ab" + "cd";
  return v.length;
}
function f3(): number {
  let v: any = "abc";
  v = "xy" + "z!";
  const w: any = v;
  return w.length;
}
function f4(): number {
  const v = "q" + "rs";
  const m = v.length;
  return m + v.length;
}
function f5(): number {
  const v = "th" + "row!";
  try {
    throw v.length;
  } catch (e) {
    return e as number;
  }
  return 0;
}
function f6(): string {
  const v = "ow" + "ned";
  return v;
}
function f7(): string {
  const s = { name: "na" + "me!" };
  return s.name;
}
function f8(): any {
  const a: any[] = ["el" + "em"];
  return a[0];
}
console.log(f1(), f2(), f3(), f4(), f5());
console.log(f6(), f7(), f8());
