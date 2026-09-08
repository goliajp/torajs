// A generator function EXPRESSION assigned into an any-typed field.
// The parser hoists it to a top-level `function* __genexpr_N` and
// leaves `Ident("__genexpr_N")` behind — a raw FnSig, which the
// member-assign lowering stored into the any slot UNBOXED
// (`store <fn_addr>, %__this +32`). The field's drop then rc_dec'd a
// code address: SIGBUS at program exit, after the program's own
// output had already printed. The destructuring-default twin of this
// exact shape was already routed through the forwarder; the
// member-assign one was not.
class SetLike {
  keys: any;
  size: number;
  has: any;
  constructor() {
    this.size = 2;
    this.keys = function* ks() {
      yield 1;
      yield 2;
    };
    this.has = function (v: any) {
      return v === 1 || v === 2;
    };
  }
}
const s = new SetLike();
const it = s.keys();
console.log(it.next().value, it.next().value, it.next().done);
console.log(s.has(1), s.has(9), s.size);

// anonymous, and never called — the crash was in the DROP, so a
// field that is only written still has to survive exit
class Held {
  g: any;
  constructor() {
    this.g = function* () {
      yield 0;
    };
  }
}
const h = new Held();
console.log("held");

// index-assign spelling, and the top-level object spelling that
// already worked
const o: any = {};
const k = "gen";
o[k] = function* () {
  yield 5;
};
console.log(o[k]().next().value);
o.direct = function* () {
  yield 6;
};
console.log(o.direct().next().value);
console.log(typeof h.g);
