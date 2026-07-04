// Any-method-call RFC 20260704 C2 — Array methods on any receivers:
// indexOf / includes / join / shift / unshift over i64 / f64 / str
// (HEAP) / mixed (Arr<Any>) element tiers.
const a: any = [1, 2, 3];
console.log(a.indexOf(2));
console.log(a.indexOf(9));
console.log(a.indexOf(2, 2));
console.log(a.includes(3));
console.log(a.includes(0));
console.log(a.join("-"));
console.log(a.join());
console.log(a.shift());
console.log(a.length);
console.log(a.unshift(0));
console.log(a[0]);
console.log(a.join("+"));
const b: any = ["x", "y"];
console.log(b.indexOf("y"));
console.log(b.includes("z"));
console.log(b.join("|"));
const c: any = [1.5, 2.5];
console.log(c.join(","));
console.log(c.includes(1.5));
const d: any = [1, "s", true];
console.log(d.indexOf("s"));
console.log(d.join("~"));
console.log(d.unshift(null));
console.log(d.length);
console.log(d.shift());
const f: any = [NaN, 1];
console.log(f.includes(NaN));
console.log(f.indexOf(NaN));
