// `<Ctor>.prototype.<m> = undefined` writes a real own property. The
// probe that reads the singleton answers ANY_UNDEF for that AND for
// an absent key, so every face that reads it has to ask membership
// too — otherwise the builtin method shows through a property whose
// value is undefined.
function show(label: string, f: () => any) {
  let out = "";
  try { out = "" + f(); } catch (e: any) { out = "THROW:" + (e && e.constructor ? e.constructor.name : "?"); }
  console.log(label + " = " + out);
}

(Array.prototype as any).join = undefined;
show("arr typeof", () => typeof (Array.prototype as any).join);
show("arr in", () => "join" in Array.prototype);
show("arr hasOwn", () => Object.prototype.hasOwnProperty.call(Array.prototype, "join"));
show("arr gopd", () => {
  const d: any = Object.getOwnPropertyDescriptor(Array.prototype, "join");
  return d ? "value=" + typeof d.value : "no-desc";
});
show("arr call", () => { const a: any = [1, 2]; return a.join(); });
show("arr toString", () => { const a: any = [1, 2]; return a.toString(); });
show("arr String()", () => { const a: any = [1, 2]; return String(a); });
(Array.prototype as any).join = Array.prototype.constructor.prototype.join;

(Map.prototype as any).get = undefined;
show("map typeof", () => typeof (Map.prototype as any).get);
show("map gopd", () => {
  const d: any = Object.getOwnPropertyDescriptor(Map.prototype, "get");
  return d ? "value=" + typeof d.value : "no-desc";
});
show("map call", () => { const m: any = new Map([[1, "a"]]); return m.get(1); });

(String.prototype as any).slice = undefined;
show("str typeof", () => typeof (String.prototype as any).slice);
show("str call", () => { const s: any = "abcd"; return s.slice(1); });

(Object.prototype as any).valueOf = undefined;
show("objproto typeof", () => typeof (Object.prototype as any).valueOf);
show("objproto gopd", () => {
  const d: any = Object.getOwnPropertyDescriptor(Object.prototype, "valueOf");
  return d ? "value=" + typeof d.value : "no-desc";
});

// An absent key still reads through to the builtin surface.
show("absent reads through", () => typeof (Array.prototype as any).map);
show("absent gopd", () => {
  const d: any = Object.getOwnPropertyDescriptor(Array.prototype, "map");
  return d ? "value=" + typeof d.value : "no-desc";
});
