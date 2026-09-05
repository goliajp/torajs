// Handing a named fn declaration whose body says `this` to `push` /
// `unshift` on a runtime-props container parks it in exactly the slot
// `z[0] = fn` would have used, and it comes back out through exactly
// the same receiver-honouring channel — so it must see the container
// as `this` when called back as a method (§13.3.6.2).
//
// Before rotation 595 every spelling below answered `undefined`: the
// store arm reached assignments only, and an argument position has no
// assignment and no key for it to look at. Eight spellings, all eight
// wrong, all exit 0.

function t(a: any) {
  return "a=" + a + "/" + typeof (this as any);
}

// the plain appender
const z: any[] = [];
z.push(t);
console.log(z[0](1));

// unshift — the other ES appender
const u: any[] = [9];
u.unshift(t);
console.log(u[0](2));

// beyond the first argument
const m: any[] = [];
m.push(1, t);
console.log(m[1](3));

// the `as any` shell erases nothing at runtime
const c: any[] = [];
c.push(t as any);
console.log(c[0](4));

// an unannotated array-literal binding is the same container
const lit = [];
lit.push(t);
console.log(lit[0](5));

// a factory-call root, admitted off the callee's declared return
function mk(): any[] {
  return [];
}
const f = mk();
f.push(t);
console.log(f[0](6));

// a keyed hop off one of those roots
const rows: any[][] = [[]];
rows[0].push(t);
console.log(rows[0][0](7));

// an `: any` binding holding an array
const anyb: any = [];
anyb.push(t);
console.log(anyb[0](8));

// the name face survives the shell, as it does on every other arm
console.log(z[0].name, z[0].length);
