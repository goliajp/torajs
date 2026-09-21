// §25.5.2.3 SerializeJSONProperty step 2 — the `toJSON` hook is
// taken when IsCallable(toJSON) is true, and §7.2.3 names three
// shapes. tr asked `call_target`, which answers a narrower
// question: which closure-shaped lane drives this cell. A callable
// Proxy and a class object have no such lane, so they read as "no
// hook".
//
// And "no hook" here does not refuse the program — it serializes
// the hook itself: `o.toJSON = new Proxy(fn, {})` produced
// `{"toJSON":{}}` where every engine produces what the hook
// returns. A wrong document out of a program that compiled and ran,
// which no gate can see.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const proxied: any = new Proxy(() => 7, {});
const trapped: any = new Proxy(() => 0, { apply: () => "T" });

console.log("dynobj proxy  ", attempt(() => {
  const o: any = {};
  o.toJSON = proxied;
  return JSON.stringify(o);
}));
// The handler decides the call (§10.5.12), and the hook's `this`
// is the object being serialized — both travel through the thunk.
console.log("dynobj trap   ", attempt(() => {
  const o: any = {};
  o.toJSON = trapped;
  return JSON.stringify(o);
}));
console.log("this receiver ", attempt(() => {
  const o: any = {};
  o.v = 5;
  o.toJSON = new Proxy(() => 0, { apply: (_t: any, th: any) => th.v * 3 });
  return JSON.stringify(o);
}));
// An array consults its expando props for the same hook.
console.log("array expando ", attempt(() => {
  const a: any = [1];
  a.toJSON = proxied;
  return JSON.stringify(a);
}));
// The hook applies at a property site too, not only at the root.
console.log("nested        ", attempt(() => {
  const o: any = {};
  o.toJSON = proxied;
  return JSON.stringify({ a: o });
}));
console.log("in array      ", attempt(() => {
  const o: any = {};
  o.toJSON = proxied;
  return JSON.stringify([o]);
}));
// Controls: an ordinary closure hook is one branch unchanged, and a
// non-callable `toJSON` is an ordinary property (step 2 skips it).
console.log("plain fn      ", attempt(() => {
  const o: any = {};
  o.toJSON = () => 9;
  return JSON.stringify(o);
}));
console.log("non-callable  ", attempt(() => {
  const o: any = {};
  o.toJSON = 5;
  return JSON.stringify(o);
}));
console.log("date builtin  ", attempt(() => JSON.stringify(new Date(0))));
// Registered, not fixed here: an object LITERAL written with a
// `toJSON` property is a struct, and the struct lane decides the
// hook in the compiler from the field's static type
// (`ssa_lower_json_stringify::to_json` wants `Type::Closure`). A
// proxy field is `Any` there, so `{ toJSON: proxied }` still
// serializes the hook — a different gap, in a different layer.
