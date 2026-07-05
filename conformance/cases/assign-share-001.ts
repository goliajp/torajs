// Chunk 564 (RFC 20260705 ledger #3, assign-ident lane) — assignment
// is a SHARE, never a move: the rhs binding keeps its stake and stays
// readable after the target rebinds, drops, or goes out of scope.
function mk(i: number): string {
  return "hello-" + i;
}

// cross-scope: inner target dies before the outer source is read
let b = mk(1);
{
  let a = mk(2);
  a = b;
  console.log(a);
}
console.log(b);

// re-assigning the same source twice (was a loud "cannot transfer")
let c = mk(3);
let d = mk(4);
c = d;
c = d;
console.log(c);
console.log(d);

// self-assign
let e = mk(5);
e = e;
console.log(e);

// member-alias rhs into another slot (was a loud "cannot transfer")
let o = { name: mk(6) };
let n = o.name;
let x = mk(7);
x = n;
console.log(x);
console.log(n);
console.log(o.name);

// any-slot assign: source survives the box and the slot's drop-old
let s8 = mk(8);
let v: any = 0;
v = s8;
v = 42;
console.log(v);
console.log(s8);

// Substr Ident rhs materializes into an owned-Str slot; view source
// stays readable
let s9 = "abcdefg";
let sv = s9.slice(2);
let t = mk(9);
t = sv;
console.log(t);
console.log(sv);
