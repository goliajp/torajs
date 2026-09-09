// A sealed element is not configurable, so deleting the LAST one — pop,
// or an ArraySetLength that shrinks — is a TypeError with the array
// untouched, while preventExtensions alone leaves both legal and a seal
// still lets `length` grow (node and bun agree). A frozen array's
// `length` is non-writable, so both directions refuse. Before this,
// tr answered every one of those with a silent mutation:
// Object.freeze([3,1,2]).length = 1 gave [3]; = 5 gave
// [3,1,2,null,null]; Object.seal([3,1,2]).pop() gave [3,1].
//
// seal × shift / splice are NOT here: the spec moves the elements first
// and only then fails to delete the last one ([1,2,2]), which tr does
// not reproduce yet — registered, and kept out of this fixture rather
// than asserted wrong.
function t(tag: string, mode: string, f: (a: any) => void, typed: boolean) {
  const a: any = typed ? ([3, 1, 2] as number[]) : ([3, 1, 2] as any[]);
  if (mode === "seal") Object.seal(a); else if (mode === "ne") Object.preventExtensions(a); else Object.freeze(a);
  try { f(a); console.log(mode, tag, typed ? "num" : "any", "NO-THROW", JSON.stringify(a)); }
  catch (e) { console.log(mode, tag, typed ? "num" : "any", "threw", (e as Error).constructor.name, JSON.stringify(a)); }
}
for (const typed of [false, true]) for (const mode of ["seal", "ne", "freeze"]) {
  t("pop", mode, (a: any) => a.pop(), typed);
  if (mode !== "seal") {
    t("shift", mode, (a: any) => a.shift(), typed);
    t("splice", mode, (a: any) => a.splice(0, 1), typed);
  }
  t("len-shrink", mode, (a: any) => { a.length = 1; }, typed);
  t("len-grow", mode, (a: any) => { a.length = 5; }, typed);
}
