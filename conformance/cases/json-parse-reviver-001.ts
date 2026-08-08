var r: string[] = [];
// reviver value replacement + this typeof + traversal order
var seen: string[] = [];
var o: any = JSON.parse("{\"a\":1,\"b\":{\"c\":2,\"d\":[10,20]}}", function(k: any, v: any) {
  seen.push(k === "" ? "(root)" : k);
  if (typeof v === "number") { return v * 2; }
  return v;
});
r.push(seen.join(","));
r.push(String(o.a), String(o.b.c), String(o.b.d[1]));
// undefined answer deletes the property
var p: any = JSON.parse("{\"keep\":1,\"drop\":2}", function(k: any, v: any) {
  if (k === "drop") { return undefined; }
  return v;
});
r.push(String(p.keep), String("drop" in p));
// non-callable reviver = unfiltered
var q: any = JSON.parse("[5]", 42 as any);
r.push(String(q[0]));
// reviver this = holder
var hh: string[] = [];
JSON.parse("{\"x\":{\"y\":1}}", function(this: any, k: any, v: any) { hh.push(typeof this); return v; });
r.push(hh.join(","));
// reviver throw propagates
try { JSON.parse("[1]", function() { throw new Error("boom"); }); r.push("no"); } catch (e) { r.push((e as Error).message); }
console.log(r.join(" | "));
