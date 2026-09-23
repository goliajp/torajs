// A declared `string | undefined` return is not a ToString site for its
// own member: `return (undefined as any)` used to hand back the
// nine-character text "undefined".
function g(): string | undefined {
  const u: any = undefined;
  return u;
}
const r = g();
console.log("ret", r === undefined, typeof r, JSON.stringify(r));
console.log("direct", g() === undefined, String(g()));

// A real string still crosses unchanged.
function h(): string | undefined {
  const v: any = "ok";
  return v;
}
console.log("live", h(), h() === undefined, typeof h());

// A literal `undefined` return takes the same spelling.
function u(): string | undefined {
  return undefined;
}
console.log("lit", u() === undefined, typeof u());

// The sentinel survives being handed on through a second boundary.
function pass(): string | undefined {
  return g();
}
console.log("pass", pass() === undefined, typeof pass());
