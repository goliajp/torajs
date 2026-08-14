// 400-01 — a closure nested in a COMPOSITE initializer (array /
// object literal) capturing its own binding: the box goes up before
// the init lowers, the mint captures the box, the declaration fills
// it (ES §9.1 — a closure captures the binding, read at call time).
const a: any = [function (): any {
  return (this as any) === a;
}, 42];
console.log(a[0](), a[1], a.length);

const s: any = {
  f: function (): any {
    return s === (this as any);
  },
  v: 7,
};
console.log(s.f() && s.v === 7);

// The same shape inside a function scope (the checker pre-declare).
function g(): any {
  const t: any = {
    f: function (): any {
      return t === (this as any);
    },
  };
  return t.f();
}
console.log(g());

// Reads through the box after the fill.
const m: any = { f: function (): any { return m.v }, v: 3 };
console.log(m.f());
