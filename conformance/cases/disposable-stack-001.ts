// RFC 20260809 B5 — DisposableStack (injected builtin): use/adopt/
// defer/dispose/disposed/move, reverse-order dispose, null skip,
// idempotent dispose, ReferenceError after disposal, TypeError on
// non-disposable/non-callable, SuppressedError aggregation, and the
// [Symbol.dispose] alias driven through a `using` declaration.
const s1 = new DisposableStack();
console.log(s1.disposed);
const ra: any = { [Symbol.dispose]() { console.log("dispose a"); } };
const rb: any = { [Symbol.dispose]() { console.log("dispose b"); } };
console.log("use-ret", s1.use(ra) === ra);
s1.use(rb);
console.log("use-null", s1.use(null));
console.log("use-undef", s1.use(undefined));
s1.dispose();
console.log(s1.disposed);
s1.dispose();

const s2 = new DisposableStack();
console.log("adopt-ret", s2.adopt(42, (v: any) => { console.log("adopt", v); }));
console.log("defer-ret", s2.defer(() => { console.log("defer"); }));
s2.dispose();

const s3 = new DisposableStack();
s3.use({ [Symbol.dispose]() { console.log("moved res"); } });
const s4 = s3.move();
console.log("s3.disposed", s3.disposed);
console.log("s4.disposed", s4.disposed);
s3.dispose();
s4.dispose();

try { s1.use(ra); } catch (e: any) { console.log("use-after:", e.name); }
try { s1.adopt(1, () => {}); } catch (e: any) { console.log("adopt-after:", e.name); }
try { s1.defer(() => {}); } catch (e: any) { console.log("defer-after:", e.name); }
try { s1.move(); } catch (e: any) { console.log("move-after:", e.name); }

const s5 = new DisposableStack();
try { s5.use(123); } catch (e: any) { console.log("use-num:", e.name); }
try { s5.adopt(1, 2); } catch (e: any) { console.log("adopt-nc:", e.name); }
try { s5.defer(null); } catch (e: any) { console.log("defer-nc:", e.name); }

const s6 = new DisposableStack();
s6.defer(() => { throw new Error("e1"); });
s6.defer(() => { throw new Error("e2"); });
try { s6.dispose(); } catch (e: any) {
  console.log("agg:", e.name, e.error.message, e.suppressed.message);
}

{
  using s7 = new DisposableStack();
  s7.defer(() => { console.log("via-using"); });
}
console.log("after-using");
