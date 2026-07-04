// Any-method-call RFC 20260704 C3b — Array higher-order methods on
// any receivers: map / filter / forEach over i64 / str (HEAP) /
// mixed (Arr<Any>) tiers, index+array callback params, capturing
// callbacks, chained results, and the catchable non-closure miss.
const a: any = [1, 2, 3];
console.log(a.map((x: number) => x * 10));
console.log(a.filter((x: number) => x > 1));
a.forEach((x: number) => console.log("fe", x));
console.log(a.map((x: number, i: number) => x + i));
const b: any = ["x", "yy"];
console.log(b.map((s: string) => s.length));
console.log(b.filter((s: string) => s.length > 1));
const scale = 5;
console.log(a.map((x: number) => x * scale));
const d: any = [1, "s", true];
console.log(d.filter((v: any) => typeof v === "number"));
d.forEach((v: any, i: number) => console.log(i, v));
console.log(a.map((x: number) => x * 2).length);
try {
  a.map(7);
} catch (e) {
  console.log("non-closure callback threw");
}
