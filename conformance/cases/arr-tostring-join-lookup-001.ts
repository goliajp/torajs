// §23.1.3.36 — `Array.prototype.toString` resolves `Get(array,
// "join")` and calls what it finds; a non-callable one sends the
// call to %Object.prototype.toString% instead. Reachable through
// `String(a)` and `a + ""` too, since ToPrimitive resolves toString
// the same way.
function t(name: string, f: () => any) {
  let out = "";
  try { out = "" + f(); } catch (e: any) { out = "THROW:" + (e && e.constructor ? e.constructor.name : "?"); }
  console.log(name + " = " + out);
}

const origJoin: any = (Array.prototype as any).join;

t("patched join", () => {
  (Array.prototype as any).join = function () { return "J"; };
  const a: any = [1, 2];
  const r = a.toString();
  (Array.prototype as any).join = origJoin;
  return r;
});
t("patched join via String()", () => {
  (Array.prototype as any).join = function () { return "J"; };
  const a: any = [1, 2];
  const r = String(a);
  (Array.prototype as any).join = origJoin;
  return r;
});
t("patched join via concat", () => {
  (Array.prototype as any).join = function () { return "J"; };
  const a: any = [1, 2];
  const r = a + "";
  (Array.prototype as any).join = origJoin;
  return r;
});
t("patched join runs once per toString", () => {
  let calls = 0;
  (Array.prototype as any).join = function () { calls++; return "J"; };
  const a: any = [1, 2, 3];
  const r = a.toString() + String(a) + calls;
  (Array.prototype as any).join = origJoin;
  return r;
});
t("own join beats proto", () => {
  (Array.prototype as any).join = function () { return "PROTO"; };
  const a: any = [1, 2];
  a.join = function () { return "OWN"; };
  const r = a.toString();
  (Array.prototype as any).join = origJoin;
  return r;
});
t("non-callable join", () => {
  (Array.prototype as any).join = 42;
  const a: any = [1, 2];
  try { return a.toString(); } finally { (Array.prototype as any).join = origJoin; }
});
t("undefined join", () => {
  (Array.prototype as any).join = undefined;
  const a: any = [1, 2];
  try { return a.toString(); } finally { (Array.prototype as any).join = origJoin; }
});
t("deleted join", () => {
  delete (Array.prototype as any).join;
  const a: any = [1, 2];
  try { return a.toString(); } finally { (Array.prototype as any).join = origJoin; }
});
t("restored join", () => {
  delete (Array.prototype as any).join;
  (Array.prototype as any).join = origJoin;
  const a: any = [1, 2];
  return a.toString();
});

class JoinArr extends Array {
  join(): any { return "SUB"; }
}
t("subclass join", () => {
  (Array.prototype as any).join = function () { return "PROTO"; };
  const a: any = new JoinArr();
  a.push(1);
  const r = a.toString();
  (Array.prototype as any).join = origJoin;
  return r;
});

// Unpatched programs are unchanged.
console.log("plain = " + [1, 2, 3].toString() + " " + String([4, 5]) + " " + ([6] + ""));
const nested: any = [[1, 2], [3]];
console.log("nested = " + nested.toString());
console.log("holes = " + [1, , 3].toString());
