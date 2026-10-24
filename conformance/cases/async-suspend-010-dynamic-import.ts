// An async body carrying a dynamic `import()` suspends over its frame
// like any other: the import's promise is awaited as any promise is.
// Both shapes here — a declaration and a value — settle in the order
// bun does: the import rejects (no such module), the catch runs, the
// body's later statements follow.
let order = "";
async function viaDecl() {
  order += "d1";
  await import("./THIS_MODULE_DOES_NOT_EXIST.js").catch(() => {
    order += "-catch";
  });
  order += "-d2";
  return order;
}
const viaValue = async () => {
  await import("./ALSO_MISSING.js").catch(() => {
    order += "-vcatch";
  });
  return "value-done";
};
viaDecl().then((r: any) => {
  console.log("decl", r);
  return viaValue();
}).then((r: any) => console.log(r, order));
