// 420-06 (§20.2.3.5) — a class constructor's toString answers the
// recorded declaration source. Hosts differ on formatting (bun
// re-prints the transpiled body), so the assertions are shape
// probes, not byte comparisons.
class C {
  static a = 1;
  m(): number {
    return 2;
  }
}
const c: any = C;
const s: string = c.toString();
console.log("starts-class:", s[0] === "c" && s[1] === "l" && s[2] === "a");
console.log("names-C:", s[6] === "C");
console.log("string-c:", String(c).length > 0);
console.log("valueOf:", c.valueOf() === c);
console.log("badge:", Object.prototype.toString.call(c));
console.log("concat:", ("" + c).length > 0);
const p: string = (Function.prototype as any).toString.call(c);
console.log("proto-call:", p[0] === "c" && p.length > 10);
