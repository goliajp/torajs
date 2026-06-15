// `const a: any[] = []; a.push(...)` — K.6 path allocates Arr<Any>
// (16-byte tagged slots via __torajs_arr_alloc_any) but K.8 push
// dispatch on a global Ident pre-fix routed through typed
// `__torajs_arr_push` (8-byte stride) + a stray
// `__torajs_rc_inc(ConstI64(1))` (Type::Any reads as refcounted),
// SIGSEGV on the next decode. Mirror of P0.10 local-Ident Any-push.

const a: any[] = [];
a.push(1);
a.push("two");
a.push(true);
a.push(null);
a.push(3.5);
console.log(a.length);
console.log(a[0]);
console.log(a[1]);
console.log(a[2]);
console.log(a[3]);
console.log(a[4]);
console.log(a.join(","));
console.log(a.join("-"));
console.log(a.join(""));

const b: any[] = [];
console.log(b.length);
console.log(b.join(","));

const c: any[] = [];
c.push(10);
c.push(20);
console.log(c.length);
console.log(c.join(","));
