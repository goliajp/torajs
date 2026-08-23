// RFC 20260823-typedarray-substrate — §10.4.5.3 / §10.4.5.5: a
// canonical numeric spelling on a typed array is the ELEMENT face,
// never the expando bag.
const t = (label: string, f: any): void => {
  try { f(); console.log(label, "ok"); }
  catch (e: any) { console.log(label, "threw", e.constructor.name); }
};
const ta: any = new Int8Array([10, 20, 30]);

// define with value stores the element
Object.defineProperty(ta, "0", { value: 7 });
console.log("define-store", ta[0], Object.keys(ta).join(","));

// string-key assign coerces and stores like an index write
ta["1"] = 42;
console.log("strkey-store", ta[1]);

// a throwing valueOf surfaces from the coercion even out of bounds
t("oob-coerce", () => { ta["9"] = { valueOf: () => { throw new RangeError("cv"); } }; });
t("strkey-coerce", () => { ta["0"] = { valueOf: () => { throw new RangeError("cv2"); } }; });

// invalid integer indexes refuse the define
t("def-oob", () => Object.defineProperty(ta, "9", { value: 1 }));
t("def-minus-zero", () => Object.defineProperty(ta, "-0", { value: 1 }));
t("def-not-integer", () => Object.defineProperty(ta, "1.5", { value: 1 }));

// attribute downgrades refuse
t("def-accessor", () => Object.defineProperty(ta, "0", { get: () => 1 }));
t("def-nonwritable", () => Object.defineProperty(ta, "0", { value: 1, writable: false }));
t("def-nonenumerable", () => Object.defineProperty(ta, "0", { value: 1, enumerable: false }));
t("def-nonconfigurable", () => Object.defineProperty(ta, "0", { value: 1, configurable: false }));

// full-attribute define succeeds
t("def-full", () => Object.defineProperty(ta, "2", { value: 9, writable: true, enumerable: true, configurable: true }));
console.log("end", ta[0], ta[1], ta[2], Object.hasOwn(ta, "-0"), Object.hasOwn(ta, "1.5"));
