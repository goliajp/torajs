// ECMA-402 §9.2.1 CanonicalizeLocaleList over the `locales` argument of
// toLocaleUpperCase / toLocaleLowerCase: `null` is a TypeError
// (ToObject), an element that is neither a string nor an object is a
// TypeError, an object element is ToString'd, a hole is skipped, and a
// primitive has no elements.
function t(f: () => string): string {
  try {
    return f();
  } catch (e) {
    return (e as any).name;
  }
}
const q: any = { toString() { return "tr"; } };
console.log(t(() => "i".toLocaleUpperCase(null as any)));
console.log(t(() => "i".toLocaleUpperCase([5] as any)));
console.log(t(() => "i".toLocaleUpperCase([q] as any)));
console.log(t(() => "i".toLocaleUpperCase([q])));
console.log(t(() => "I".toLocaleLowerCase([, "tr"] as any)));
console.log(t(() => "i".toLocaleUpperCase(7 as any)));
console.log(t(() => "i".toLocaleUpperCase({ length: 1, 0: "tr" } as any)));
console.log(t(() => "i".toLocaleUpperCase(["!!"])));
const loc: any = "tr";
console.log(t(() => "i".toLocaleUpperCase(loc)), t(() => "i".toLocaleUpperCase(undefined)));
