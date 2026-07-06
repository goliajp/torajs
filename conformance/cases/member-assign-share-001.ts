// chunk 566 — member assignment is a share, not a move (RFC 20260705 ledger #3):
// the slot/bucket takes its own +1 and the source binding keeps its stake,
// so re-assign drop-old no longer steals the source's only ref (UAF) and
// props lanes no longer orphan it (leak).

// 1. struct field re-assign: source stays readable after drop-old
let o = { s: "init" };
let a = "AAAA" + 1;
o.s = a;
o.s = "BBBB" + 2;
let r1 = "CCCC" + 3; // reuse-window canary — lands on a's cell if it was freed
console.log(a);
console.log(o.s);
console.log(r1);

// 2. accessor setter re-assign
class C {
  _v: string = "";
  set value(n: string) { this._v = n; }
  get value(): string { return this._v; }
}
let c = new C();
let b = "DDDD" + 4;
c.value = b;
c.value = "EEEE" + 5;
let r2 = "FFFF" + 6;
console.log(b);
console.log(c.value);
console.log(r2);

// 3. member rhs share: o.s = p.q keeps p.q alive across overwrite
let p = { q: "gggg" + 7 };
o.s = p.q;
o.s = "HHHH" + 8;
console.log(p.q);
console.log(o.s);

// 4. array props: ident + owned-temp overwrite, source readable
let arr = [1, 2, 3];
let s4 = "IIII" + 9;
arr.x = s4;
arr.x = "JJJJ" + 10;
console.log(s4);
console.log(arr.x);

// 5. closure props
let f = (n: number) => n + 1;
let s5 = "KKKK" + 11;
f.tag = s5;
f.tag = "LLLL" + 12;
console.log(s5);
console.log(f.tag);
console.log(f(1));

// 6. any-receiver dynobj member share
let d: any = {};
let s6 = "MMMM" + 13;
d.p = s6;
d.p = "NNNN" + 14;
console.log(s6);
console.log(d.p);

// 7. alias-rhs write (was a loud "cannot transfer" rejection)
let box7 = { t: "OOOO" + 15 };
let al = box7.t;
o.s = al;
console.log(o.s);
console.log(al);
console.log(box7.t);
