// An async body carrying a dynamic `import()` keeps the synchronous
// lowering: the imported module's own top-level await and the async
// driver's resumption have not been made to agree, and a body that
// suspended before a module it imported reached its `await` never
// resumed (test262 top-level-await/dynamic-import-of-waiting-module,
// the one pass regression blade 2 left). Both shapes here — a
// declaration and a value — still settle in the order bun does: the
// import rejects (no such module), the catch runs, the body's later
// statements follow.
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
