// §23.1.3 forEach / map / filter / reduce / reduceRight: `len` is read
// once, but every step asks HasProperty(O, k) NOW and reads with
// [[Get]] — a callback (or a getter) that shrinks `length`, deletes a
// later index, or defines an accessor on one changes what the loop
// visits and what it reads (test262 Array/prototype/*/7-b, 9-b, 9-c-i).
function log(label: string, xs: any[]) { console.log(label, xs.join(",")); }
// a getter on 0 shrinks the array while it is being read
{
  const seen: any[] = [];
  const arr: any[] = [0, 1, 2, "last"];
  Object.defineProperty(arr, "0", { get: function () { arr.length = 3; return 0; }, configurable: true });
  arr.forEach(function (v: any, i: any) { seen.push(i); });
  log("getter-shrink", seen); console.log(arr.length);
}
// the callback shrinks the array
{
  const seen: any[] = [];
  const arr: any[] = [0, 1, 2, "last"];
  arr.forEach(function (v: any, i: any) { if (i === 0) arr.length = 3; seen.push(i); });
  log("cb-shrink", seen);
}
// the callback deletes a later index
{
  const seen: any[] = [];
  const arr: any[] = [0, 1, 2, "last"];
  const r = arr.map(function (v: any, i: any) { if (i === 0) delete arr[2]; seen.push(i); return v; });
  log("cb-delete", seen); console.log(r.length, 2 in r, r[3]);
}
// the callback defines a getter on a later index — the loop reads it
{
  const seen: any[] = [];
  const arr: any[] = [0, 1, 2, "last"];
  arr.forEach(function (v: any, i: any) {
    if (i === 0) Object.defineProperty(arr, "2", { get: function () { return "got"; }, configurable: true });
    seen.push(v);
  });
  log("cb-getter", seen);
}
// reduce / reduceRight under the same mutations
{
  const arr: any[] = [1, 2, 3, 4];
  const s = arr.reduce(function (acc: any, v: any, i: any) { if (i === 0) delete arr[2]; return acc + "," + v; }, "");
  console.log("reduce-delete", s);
  const arr2: any[] = [1, 2, 3, 4];
  const s2 = arr2.reduceRight(function (acc: any, v: any, i: any) { if (i === 3) arr2.length = 2; return acc + "," + v; }, "");
  console.log("reduceRight-shrink", s2);
}
// filter keeps the read value of a getter-defined element
{
  const arr: any[] = [1, 2, 3];
  Object.defineProperty(arr, "1", { get: function () { return 20; }, configurable: true });
  console.log("filter-getter", arr.filter(function (v: any) { return v > 2; }).join(","));
}
// a throwing getter propagates out of the loop
{
  const arr: any[] = [1, 2, 3];
  Object.defineProperty(arr, "1", { get: function () { throw new Error("boom"); }, configurable: true });
  try { arr.forEach(function () {}); console.log("no throw"); } catch (e) { console.log("threw", (e as any).message); }
}
// a dense boxed array with no mutation still walks every element
{
  const arr: any[] = [1, "a", null, undefined, 5];
  console.log("dense", arr.map(function (v: any) { return typeof v; }).join(","));
}
