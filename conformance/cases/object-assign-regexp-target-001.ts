// Object.assign onto a RegExp (§20.1.2.1): the sources' own enumerable
// properties land on the RegExp itself, which the call answers. An own
// `exec` copied this way is what RegExpExec then calls.
const r = /a/g;
const out = Object.assign(r, { tag: "t", lastIndex: 2 });
console.log(out === r, (r as any).tag, r.lastIndex);
console.log(r.test("aaa"), r.lastIndex);
const s = /b/;
Object.assign(s as any, { exec: () => null });
console.log("abc".replace(s, "X"), "abc".search(s));
const t = /c/;
Object.assign(t, { extra: 1 }, { more: [1, 2] });
console.log(Object.keys(t).join(","), t.test("c"));
for (let i = 0; i < 3; i++) {
  const fresh = Object.assign(/x/, { n: i });
  console.log((fresh as any).n, fresh.source);
}
try {
  Object.assign(new RegExp("q"), {
    get a(): number {
      throw new Error("boom");
    },
  });
} catch (e) {
  console.log((e as Error).message);
}
