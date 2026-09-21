// The rest of the higher-order gates, asking the same §7.2.3
// question the array ones now ask. Each one had its own copy of the
// closure-cell probe in front of its own loop, so each one turned
// away the two callable shapes that have no boxed pair.
//
// %TypedArray%.prototype (§23.2.3.x step 3), Array.prototype's
// generic array-like lane (§23.1.3.x over an object with a length),
// and Map / Set forEach (§24.1.3.5, §24.2.4.7 step 3).
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const dbl: any = new Proxy(((x: number) => x * 2) as any, {});
const big: any = new Proxy(((x: number) => x > 1) as any, {});
const sum: any = new Proxy(((a: number, b: number) => a + b) as any, {});
const desc: any = new Proxy(((a: number, b: number) => b - a) as any, {});
const show = (t: any): string => Array.from(t).join(",");

// %TypedArray%.prototype
console.log("TA map      ", attempt(() => show(new Uint8Array([1, 2]).map(dbl))));
console.log("TA filter   ", attempt(() => show(new Uint8Array([1, 2, 3]).filter(big))));
console.log("TA find     ", attempt(() => (new Uint8Array([1, 2, 3]) as any).find(big)));
console.log("TA every    ", attempt(() => (new Uint8Array([1, 2]) as any).every(big)));
console.log("TA some     ", attempt(() => (new Uint8Array([1, 2]) as any).some(big)));
console.log("TA reduce   ", attempt(() => (new Uint8Array([1, 2, 3]) as any).reduce(sum, 0)));
console.log("TA forEach  ", attempt(() => {
  const o: number[] = [];
  (new Uint8Array([1, 2]) as any).forEach(new Proxy(((x: number) => { o.push(x); }) as any, {}));
  return o.join(",");
}));
console.log("TA sort     ", attempt(() => show((new Uint8Array([1, 3, 2]) as any).sort(desc))));
console.log("TA toSorted ", attempt(() => show((new Uint8Array([1, 3, 2]) as any).toSorted(desc))));
console.log("TA sort dflt", attempt(() => show((new Uint8Array([3, 1, 2]) as any).sort())));
console.log("TA plain    ", attempt(() => show(new Uint8Array([1, 2]).map((x: number) => x * 2))));
console.log("TA refuses  ", attempt(() => (new Uint8Array([1]) as any).map(5)));

// Map / Set
console.log("Map forEach ", attempt(() => {
  const o: string[] = [];
  (new Map([["a", 1], ["b", 2]]) as any)
    .forEach(new Proxy(((v: any, k: any) => { o.push(k + "=" + v); }) as any, {}));
  return o.join(",");
}));
console.log("Set forEach ", attempt(() => {
  const o: number[] = [];
  (new Set([1, 2]) as any).forEach(new Proxy(((v: number) => { o.push(v); }) as any, {}));
  return o.join(",");
}));
console.log("Map plain   ", attempt(() => {
  const o: string[] = [];
  (new Map([["a", 1]]) as any).forEach((v: any, k: any) => o.push(k + "=" + v));
  return o.join(",");
}));
console.log("Map refuses ", attempt(() => (new Map() as any).forEach(5)));

// Array.prototype's generic array-like lane
const like: any = { 0: 1, 1: 3, 2: 2, length: 3 };
console.log("AL map      ", attempt(() => Array.prototype.map.call(like, dbl).join(",")));
console.log("AL filter   ", attempt(() => Array.prototype.filter.call(like, big).join(",")));
console.log("AL reduce   ", attempt(() => (Array.prototype.reduce as any).call(like, sum, 0)));
console.log("AL toSorted ", attempt(() => (Array.prototype as any).toSorted.call(like, desc).join(",")));
console.log("AL sort     ", attempt(() => (Array.prototype.sort as any).call({ ...like }, desc).length));
console.log("AL plain    ", attempt(() =>
  Array.prototype.map.call(like, (x: number) => x * 2).join(",")));
console.log("AL refuses  ", attempt(() => Array.prototype.map.call(like, 5 as any)));
// A primitive receiver's sort gate asks the same question.
console.log("prim sort   ", attempt(() => (Array.prototype.sort as any).call("abc", 5)));
// A class object is callable everywhere, and refused when called.
class K {}
console.log("TA class    ", attempt(() => (new Uint8Array([1]) as any).map(K)));
console.log("Map class   ", attempt(() => (new Map([["a", 1]]) as any).forEach(K)));
console.log("AL class    ", attempt(() => Array.prototype.map.call(like, K as any)));
