// §10.1.8.1 OrdinaryGet order on the two receivers that carry own
// properties: an array's side props and a closure's, then the
// subclass link, then the builtin prototype. A patch installed on
// `Array.prototype` / `Function.prototype` has to be what the call
// resolves to whenever nothing below it answers first.
function t(name: string, f: () => any) {
  let out = "";
  try { out = "" + f(); } catch (e: any) { out = "THROW:" + (e && e.constructor ? e.constructor.name : "?"); }
  console.log(name + " = " + out);
}

const origPush: any = (Array.prototype as any).push;
const origSlice: any = (Array.prototype as any).slice;
const origCall: any = (Function.prototype as any).call;
const origBind: any = (Function.prototype as any).bind;

t("arr patch", () => {
  (Array.prototype as any).push = function () { return "PATCHED"; };
  const a: any = [];
  const r = a.push(1);
  (Array.prototype as any).push = origPush;
  return r;
});
t("arr own beats patch", () => {
  (Array.prototype as any).push = function () { return "PROTO"; };
  const a: any = [];
  a.push = function () { return "OWN"; };
  const r = a.push(1);
  (Array.prototype as any).push = origPush;
  return r;
});
t("arr own undefined beats patch", () => {
  (Array.prototype as any).push = function () { return "PROTO"; };
  const a: any = [];
  a.push = undefined;
  try { return a.push(1); } finally { (Array.prototype as any).push = origPush; }
});
t("arr patch non-callable", () => {
  (Array.prototype as any).push = 42;
  const a: any = [];
  try { return a.push(1); } finally { (Array.prototype as any).push = origPush; }
});
t("arr patch undefined", () => {
  (Array.prototype as any).push = undefined;
  const a: any = [];
  try { return a.push(1); } finally { (Array.prototype as any).push = origPush; }
});
t("arr delete", () => {
  delete (Array.prototype as any).slice;
  const a: any = [1, 2];
  try { return a.slice(0); } finally { (Array.prototype as any).slice = origSlice; }
});
t("arr patch via defineProperty", () => {
  Object.defineProperty(Array.prototype, "push", {
    value: function () { return "DEFINED"; },
    writable: true, enumerable: false, configurable: true,
  });
  const a: any = [];
  const r = a.push(1);
  (Array.prototype as any).push = origPush;
  return r;
});

class MyArr extends Array {
  push(): any { return "SUB"; }
}
t("arr subclass beats patch", () => {
  (Array.prototype as any).push = function () { return "PROTO"; };
  const a: any = new MyArr();
  const r = a.push(1);
  (Array.prototype as any).push = origPush;
  return r;
});

t("fn patch", () => {
  (Function.prototype as any).call = function () { return "PATCHED"; };
  const f: any = function () { return "NATIVE"; };
  const r = f.call(null);
  (Function.prototype as any).call = origCall;
  return r;
});
t("fn own beats patch", () => {
  (Function.prototype as any).call = function () { return "PROTO"; };
  const f: any = function () { return "NATIVE"; };
  f.call = function () { return "OWN"; };
  const r = f.call(null);
  (Function.prototype as any).call = origCall;
  return r;
});
t("fn patch non-callable", () => {
  (Function.prototype as any).call = 42;
  const f: any = function () { return "NATIVE"; };
  try { return f.call(null); } finally { (Function.prototype as any).call = origCall; }
});
t("fn delete", () => {
  delete (Function.prototype as any).bind;
  const f: any = function () { return "NATIVE"; };
  try { return f.bind(null); } finally { (Function.prototype as any).bind = origBind; }
});

// The §20.1.3.5 leg reaches these two families now: an array
// redefines toLocaleString, a function inherits it.
t("arr toLocaleString ignores toString patch", () => {
  const orig: any = (Array.prototype as any).toString;
  (Array.prototype as any).toString = function () { return "T"; };
  const a: any = [1, 2];
  const r = a.toLocaleString();
  (Array.prototype as any).toString = orig;
  return r;
});
t("fn toLocaleString follows toString patch", () => {
  const orig: any = (Function.prototype as any).toString;
  (Function.prototype as any).toString = function () { return "T"; };
  const f: any = function () {};
  const r = f.toLocaleString();
  (Function.prototype as any).toString = orig;
  return r;
});

// Nothing above changes an unpatched program.
const plain: any = [3, 1, 2];
console.log("plain = " + plain.slice(0).sort().join("-") + " " + plain.length);
