// a binding is read through the scopes around the site: same-named
// bindings of different annotated types in different functions, blocks,
// loop heads, catch clauses and the module each type their own arrows
const fs: Array<(n: number) => number> = [];
fs.push((n) => n + 1);
function strs(): string {
  const fs: Array<(s: string) => string> = [];
  fs.push((s) => s + "!");
  const run = () => {
    // the arrow reads the enclosing function's fs, not the module's
    fs.push((s) => s.toUpperCase());
    return fs.map((f) => f("a")).join(",");
  };
  return run();
}
function blocks(): string {
  const out: string[] = [];
  {
    const gs: Array<(n: number) => number> = [];
    gs.push((n) => n * 2);
    out.push(String(gs[0](21)));
  }
  {
    const gs: Array<(s: string) => number> = [];
    gs.push((s) => s.length);
    out.push(String(gs[0]("four")));
  }
  for (let i = 0; i < 1; i++) {
    const hs: Array<(b: boolean) => string> = [];
    hs.push((b) => (b ? "yes" : "no"));
    out.push(hs[0](true));
  }
  return out.join(",");
}
function loopNums(): string {
  const fs: Array<() => number> = [];
  for (let i = 0; i < 3; i++) {
    const n = i * 10;
    fs.push(() => n);
  }
  return fs.map((f) => f()).join(",");
}
function loopStrs(): string {
  const fs: Array<() => string> = [];
  for (const s of ["p", "q"]) {
    const t = s.toUpperCase();
    fs.push(() => t);
  }
  return fs.map((f) => f()).join(",");
}
function caught(): string {
  try {
    throw new Error("x");
  } catch (fs) {
    return typeof fs;
  }
}
console.log(fs[0](41));
console.log(strs());
console.log(blocks());
console.log(loopNums());
console.log(loopStrs());
console.log(caught());
