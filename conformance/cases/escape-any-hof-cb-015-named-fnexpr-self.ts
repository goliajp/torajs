// A named function expression reading its own name passes itself, not
// the top-level declaration spelled the same
function show(v: number): string {
  return "G" + v;
}

const w = function show(v: any): string {
  const a: any[] = Array.isArray(v) ? v : [];
  return a.length ? "[" + a.map(show).join(",") + "]" : typeof v + ":" + v;
};
console.log(w([1, "s", [2, true]]));

const k = function show(v: any): string {
  const a: any[] = Array.isArray(v) ? v : [];
  let n = 0;
  a.forEach(show);
  a.forEach((x: any) => {
    n = n + 1;
  });
  return a.length ? "k" + n : "v" + typeof v;
};
console.log(k([1, "s"]));

const d: number[] = [1, 2];
const u: any = d;
u[0] = "t";
console.log(d.map(show).join(","), show(3));
