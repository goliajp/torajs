// Putting a builtin method back after `delete <Ctor>.prototype.<m>`
// revives it — including when what you put back is the ORIGINAL
// method rather than a replacement. §10.1.8.1 resolves the own entry
// that is there now; a delete that has been undone has nothing left
// to say.
function t(name: string, f: () => any) {
  let out = "";
  try { out = "" + f(); } catch (e: any) { out = "THROW:" + (e && e.constructor ? e.constructor.name : "?"); }
  console.log(name + " = " + out);
}

t("map original", () => {
  const orig: any = (Map.prototype as any).get;
  delete (Map.prototype as any).get;
  (Map.prototype as any).get = orig;
  const m: any = new Map([[1, "a"]]);
  return m.get(1);
});
t("map replacement", () => {
  const orig: any = (Map.prototype as any).get;
  delete (Map.prototype as any).get;
  (Map.prototype as any).get = function () { return "USER"; };
  const m: any = new Map([[1, "a"]]);
  const r = m.get(1);
  (Map.prototype as any).get = orig;
  return r;
});
t("map still deleted", () => {
  const orig: any = (Map.prototype as any).has;
  delete (Map.prototype as any).has;
  const m: any = new Map([[1, "a"]]);
  try { return m.has(1); } finally { (Map.prototype as any).has = orig; }
});
t("map deleted-then-restored", () => {
  const m: any = new Map([[1, "a"]]);
  return m.has(1);
});

t("str original", () => {
  const orig: any = (String.prototype as any).slice;
  delete (String.prototype as any).slice;
  (String.prototype as any).slice = orig;
  const s: any = "abcd";
  return s.slice(1);
});
t("set original", () => {
  const orig: any = (Set.prototype as any).add;
  delete (Set.prototype as any).add;
  (Set.prototype as any).add = orig;
  const s: any = new Set();
  s.add(7);
  return s.has(7);
});
t("date original", () => {
  const orig: any = (Date.prototype as any).getTime;
  delete (Date.prototype as any).getTime;
  (Date.prototype as any).getTime = orig;
  const d: any = new Date(1234);
  return d.getTime();
});
t("regexp original", () => {
  const orig: any = (RegExp.prototype as any).test;
  delete (RegExp.prototype as any).test;
  (RegExp.prototype as any).test = orig;
  const re: any = /ab/;
  return re.test("xaby");
});
t("restore via defineProperty", () => {
  const orig: any = (Map.prototype as any).delete;
  delete (Map.prototype as any).delete;
  Object.defineProperty(Map.prototype, "delete", {
    value: orig, writable: true, enumerable: false, configurable: true,
  });
  const m: any = new Map([[1, "a"]]);
  return m.delete(1);
});
