// §24.1.3.5 step 3 / §24.2.4.7 step 3 — Map and Set `forEach` ask
// IsCallable of the callback, and §7.2.3 names three shapes. The
// any-lane arm in `method_call_mapset` has resolved all three since
// rotation 682; the checker's callback-widening arm only knew Array
// receivers, so a Map or Set handed a callable Proxy — or any value
// whose static type is `any` — was refused before reaching it.
//
// That left the two halves of one slot disagreeing: `m.forEach({})`
// had just learned to throw (the spec's answer) while
// `m.forEach(anyCallback)` still would not compile.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
let out = "";
const m = new Map<any, any>([[1, 2], [3, 4]]);
const s = new Set<any>([5, 6]);
const proxied: any = new Proxy((v: any, k: any) => {
  out += k + ":" + v + " ";
}, {});
const anyCb: any = (v: any, k: any) => {
  out += k + "=" + v + " ";
};
class K {}

console.log("map proxy cb  ", attempt(() => {
  out = "";
  m.forEach(proxied);
  return out.trim();
}));
console.log("set proxy cb  ", attempt(() => {
  out = "";
  s.forEach(proxied);
  return out.trim();
}));
console.log("map any cb    ", attempt(() => {
  out = "";
  m.forEach(anyCb);
  return out.trim();
}));
console.log("set any cb    ", attempt(() => {
  out = "";
  s.forEach(anyCb);
  return out.trim();
}));
// A class object is callable, so it passes step 3 and then §10.2.1
// step 2 refuses the call — a TypeError, not a compile refusal.
console.log("map class cb  ", attempt(() => m.forEach(K as any)));
// Not callable at all: step 3 throws. Both spellings agree, which
// is the point — one is decided while compiling, one at run time.
console.log("map noncall   ", attempt(() => m.forEach({})));
console.log("map noncall/any", attempt(() => m.forEach({} as any)));
console.log("set noncall   ", attempt(() => s.forEach(5)));
// The typed callback keeps its typed lane.
console.log("map typed cb  ", attempt(() => {
  let r = "";
  m.forEach((v: any, k: any) => {
    r += k;
  });
  return r;
}));
console.log("set typed cb  ", attempt(() => {
  let r = "";
  s.forEach((v: any) => {
    r += v;
  });
  return r;
}));
// A Set's callback sees the value twice (§24.2.4.7 step 4.a.i).
console.log("set arg pair  ", attempt(() => {
  out = "";
  s.forEach(anyCb);
  return out.trim();
}));
