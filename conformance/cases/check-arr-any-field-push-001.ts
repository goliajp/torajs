// `class W { items: any[] = [] }` / ctor `this.items = []`
// allocates the array via __torajs_arr_alloc_any (16B tagged
// slots), but the (b) struct-field receiver push arm pre-fix
// fell through to typed __torajs_arr_push (8B stride) +
// stray rc_inc(ConstI64(1)) (Type::Any reads as refcounted),
// wrote raw int into the Any slot, and SIGSEGV'd on the next
// decode. Mirror of the local-Ident / K.8 const-global fixes.

class W {
  items: any[];
  constructor() {
    this.items = [];
  }
}

const w = new W();
w.items.push(1);
w.items.push("two");
w.items.push(true);
w.items.push(null);
w.items.push(2.5);
console.log(w.items.length);
console.log(w.items[0]);
console.log(w.items[1]);
console.log(w.items[2]);
console.log(w.items[3]);
console.log(w.items[4]);
console.log(w.items.join(","));

class V {
  tag: string;
  xs: any[];
  constructor(t: string) {
    this.tag = t;
    this.xs = [];
  }
}

const v = new V("hello");
v.xs.push(10);
v.xs.push("world");
console.log(v.tag);
console.log(v.xs.length);
console.log(v.xs.join("|"));
