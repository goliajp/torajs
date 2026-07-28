// p1 — Symbol key getter (the 557-case true shape)
var obj = {
  get [Symbol.iterator]() { return 42; }
};
console.log(obj[Symbol.iterator]);

// p2 — string computed getter + setter, key evaluation order
var order: string[] = [];
var k = (n: string) => { order.push(n); return n; };
var o2 = {
  get [k("a")]() { return 1; },
  set [k("b")](v: number) { order.push("set:" + v); }
};
console.log(o2.a);
o2.b = 7;
console.log(order.join(","));

// p3 — getter throws (async-generator cluster shape)
var reason = { msg: "boom" };
var o3 = {
  get [Symbol.asyncIterator]() { throw reason; }
};
try { o3[Symbol.asyncIterator]; } catch (e) { console.log((e as any).msg); }

// p4 — literal-string whole key folds to the static accessor slot
var o4 = { get ["s"]() { return "st"; } };
console.log(o4.s);

// p5 — get+set same computed key merge into one accessor pair
var store = 0;
var kk = "v";
var o5 = {
  get [kk]() { return store; },
  set [kk](x: number) { store = x + 1; }
};
o5.v = 4;
console.log(o5.v);
