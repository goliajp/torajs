// §10.1.6.3 precondition on a view's expando defines: a
// non-extensible receiver refuses a NEW bag key; an in-bounds
// ELEMENT define stays valid (elements are not bag entries).
const t = (label: string, f: any): void => {
  try { f(); console.log(label, "ok"); }
  catch (e: any) { console.log(label, "threw", e.constructor.name); }
};
const ta: any = new Int8Array([1, 2]);
Object.defineProperty(ta, "kept", { value: 1, configurable: true });
Object.preventExtensions(ta);
t("new-key", () => Object.defineProperty(ta, "fresh", { value: 2 }));
t("existing-key", () => Object.defineProperty(ta, "kept", { value: 3, configurable: true }));
t("element", () => Object.defineProperty(ta, "0", { value: 9 }));
console.log("state", ta.kept, ta[0], Object.hasOwn(ta, "fresh"));

const ab: any = new ArrayBuffer(4);
Object.preventExtensions(ab);
t("ab-new-key", () => Object.defineProperty(ab, "x", { value: 1 }));
t("ab-assign", () => { ab.y = 2; });
console.log("ab", Object.hasOwn(ab, "x"), ab.y);
