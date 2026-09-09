// A function's virtual own pair (`name` / `length`) and its lazily
// minted `prototype` have no bag entry to carry the attribute bits
// `Object.seal` / `Object.freeze` clear, so the descriptor is read off
// the header: sealed or frozen makes name / length non-configurable,
// frozen makes prototype non-writable. And a plain function's own
// `prototype` is {[[Configurable]]: false} from the start (§20.2.4.3),
// so `delete f.prototype` refuses whether or not anything was frozen;
// an arrow function owns none and the delete is a spec success.
function t(tag: string, f: () => unknown) {
  try { console.log(tag, "->", JSON.stringify(f())); } catch (e) { console.log(tag, "threw", (e as Error).constructor.name, (e as Error).message); }
}
t("plain fn name desc", () => { const f: any = function foo() {}; return Object.getOwnPropertyDescriptor(f, "name"); });
t("sealed fn name desc", () => { const f: any = function foo() {}; Object.seal(f); return Object.getOwnPropertyDescriptor(f, "name"); });
t("frozen fn name desc", () => { const f: any = function foo() {}; Object.freeze(f); return Object.getOwnPropertyDescriptor(f, "name"); });
t("frozen fn length desc", () => { const f: any = function (a: number, b: number) { return a + b; }; Object.freeze(f); return Object.getOwnPropertyDescriptor(f, "length"); });
t("plain fn prototype desc", () => { const f: any = function () {}; const d = Object.getOwnPropertyDescriptor(f, "prototype"); return [d?.writable, d?.enumerable, d?.configurable]; });
t("sealed fn prototype desc", () => { const f: any = function () {}; Object.seal(f); const d = Object.getOwnPropertyDescriptor(f, "prototype"); return [d?.writable, d?.enumerable, d?.configurable]; });
t("frozen fn prototype desc", () => { const f: any = function () {}; Object.freeze(f); const d = Object.getOwnPropertyDescriptor(f, "prototype"); return [d?.writable, d?.enumerable, d?.configurable]; });
t("frozen arrow name desc", () => { const f: any = () => 1; Object.freeze(f); return Object.getOwnPropertyDescriptor(f, "name"); });
t("delete plain fn prototype", () => { const f: any = function () {}; return [delete f.prototype]; });
t("delete frozen fn prototype", () => { const f: any = function () {}; Object.freeze(f); return [delete f.prototype]; });
t("delete arrow prototype", () => { const f: any = () => 1; return [delete f.prototype, "prototype" in f]; });
t("delete method prototype", () => { const o = { m() {} }; return [delete (o.m as any).prototype]; });
t("delete class prototype", () => { class D {} return [delete (D as any).prototype]; });
t("delete fn name then desc", () => { const f: any = function foo() {}; return [delete f.name, Object.getOwnPropertyDescriptor(f, "name"), f.name]; });
t("sealed fn prototype write", () => { const f: any = function () {}; Object.seal(f); f.prototype = { z: 1 }; return [f.prototype.z]; });
t("frozen fn prototype write", () => { const f: any = function () {}; Object.freeze(f); f.prototype = { z: 1 }; return 1; });
t("frozen fn Reflect.set prototype", () => { const f: any = function () {}; Object.freeze(f); return [Reflect.set(f, "prototype", 1)]; });
t("Reflect.deleteProperty plain fn prototype", () => { const f: any = function () {}; return [Reflect.deleteProperty(f, "prototype"), typeof f.prototype]; });
t("frozen fn ownKeys", () => { const f: any = function foo() {}; Object.freeze(f); return Reflect.ownKeys(f); });
