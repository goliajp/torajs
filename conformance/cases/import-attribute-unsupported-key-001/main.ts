// An import attribute key the host does not support fails the load
// (AllImportAttributesSupported): a static import carrying one is a
// SyntaxError of the importing module, which an import() of that module
// surfaces as a SyntaxError rejection, while import()'s own options
// carrying one reject with a TypeError (§13.3.10 EvaluateImportCall).
// An empty clause asks for nothing and loads. bun ignores unknown keys
// and loads both, which is why `main.expected` pins the output.
import { v } from "./lib.ts" with {};
function show(tag: string, p: Promise<any>): Promise<void> {
  return p.then((m: any) => { console.log(tag, "ok", m.w ?? m.v); },
                (e: any) => { console.log(tag, "reject", e.constructor.name); });
}
async function main(): Promise<void> {
  console.log("empty", v);
  await show("static", import("./attr.ts"));
  await show("dynamic", import("./lib.ts", { with: { foo: "1" } }));
  await show("string-key", import("./lib.ts", { with: { "k-1": "a" } }));
}
main();
