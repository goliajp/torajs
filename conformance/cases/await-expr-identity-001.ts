// `await e` in expression position dispatches by TYPE (§27.7.5.1):
// Promise(T) unwraps to T, every other operand passes through identity
// — never a field lookup. Before the parser marked its minted `.value`
// reads (Ast::await_value_reads), a Struct receiver with a `value`
// field won the lookup: `await {value: 1}` answered `1` (silent
// wrong). Covers: bare struct literal, struct binding, struct WITH a
// value field via Promise (unwrap still reads the settled value),
// primitives, arrays, class async method + async arrow bodies, and a
// user's real `.value` field read AFTER an identity await.
async function main() {
  const a = await {value: 1};
  console.log("a", a);
  const obj = {value: 42, done: false};
  const b = await obj;
  console.log("b", b.value, b.done);
  const c = await Promise.resolve({value: 7});
  console.log("c", c);
  const d = await 5;
  console.log("d", d);
  const arr = await [1, 2];
  console.log("e", arr[1]);
  const s = await {value: "str"};
  console.log("f", s.value);
  const x = await (await Promise.resolve({value: 5}));
  console.log("g", x.value);
}
class C {
  async m() {
    const r = await {value: 10, done: true};
    console.log("h", r.value, r.done);
    return r.value as number;
  }
}
// serialized after main() so the two async bodies never interleave
// (tr's identity await is synchronous — the spec's one-tick suspend
// is a separate, pre-existing scheduling gap).
main().then(() => { new C().m().then((v) => console.log("i", v)); });
