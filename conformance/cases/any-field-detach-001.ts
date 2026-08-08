// RFC 20260809 B5 前置 — a nominal-struct `any` field read answers
// an OWNED value (producer-side mint): `const st: any = this.__s;
// this.__s = [];` must leave `st` alive — the slot's drop-old used to
// steal the borrow's only stake (freed array under a live binding,
// closure underflow in the at-exit cycle drain).
class D {
  __s: any = [];
  take(): any {
    const st: any = this.__s;
    this.__s = [];
    return st;
  }
}
function go(): void {
  const d = new D();
  d.__s.push({ v: 7, d: () => { console.log("dispose 7"); }, k: 4 });
  const st = d.take();
  const r = st[0];
  r.d();
  console.log(r.v, r.k);

  // Detach outside a method (instance-receiver spelling), and the
  // detached view surviving a second overwrite.
  const e = new D();
  e.__s.push(1);
  const view: any = e.__s;
  e.__s = [2];
  e.__s = [3];
  console.log(view[0], e.__s[0]);
}
go();
