// `undefined` written through an any view into a typed element stores the
// element type's own undefined — not the 0 a NULL made in the F64 lane.
const b = [1.5, 2.5]; (b as any)[1] = undefined; console.log(b[1], b, 1 in b, b.indexOf(undefined as any));
const s = ["x", "y"]; (s as any)[0] = undefined; console.log(s[0], s, typeof s[0]);
const o = [{ k: 1 }]; (o as any)[0] = undefined; console.log(o[0], o.length);
const c = [1.5]; (c as any)[3] = undefined; console.log(c, c.length, 3 in c, 2 in c);
