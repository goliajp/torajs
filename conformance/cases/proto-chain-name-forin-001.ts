// RFC 20260721-object-descriptor-cluster 刀 5 (R-F 浅层) — `.name`
// reads through the user [[Prototype]] chain, and for-in enumerates
// inherited enumerable keys with own-key shadowing (§14.7.5.9).
var a: any = {};
Object.defineProperty(a, "name", { value: "NAME", writable: true, enumerable: true, configurable: true });
var m: any = Object.create(a);
var tm: any = Object.create(m);
console.log("name depth1:", m.name, "depth2:", tm.name);
var c: any = {};
c.name = "CN";
var mc: any = Object.create(c);
console.log("plain-assign name depth1:", mc.name);
// stored-undefined own shadow answers undefined, not the chain
var s: any = Object.create(a);
s.name = undefined;
console.log("stored-undef shadow:", s.name);
// for-in walks the chain with shadowing
var base: any = { x: 1, y: 2 };
var mid: any = Object.create(base);
mid.y = 20;
mid.z = 3;
var top2: any = Object.create(mid);
top2.w = 4;
var seen: any = [];
for (var k in top2) seen.push(k);
console.log("for-in chain:", JSON.stringify(seen));
// a non-enumerable own key shadows an inherited enumerable one
var p: any = { q: 1 };
var child: any = Object.create(p);
Object.defineProperty(child, "q", { value: 2, enumerable: false });
var seen2: any = [];
for (var k2 in child) seen2.push(k2);
console.log("non-enum shadow:", JSON.stringify(seen2), child.q);
