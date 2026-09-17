// §26.1.1.1 / §26.2.1.1 — WeakRef and FinalizationRegistry are
// subclassable; `super(...)` performs the constructor's checks.
class Ref extends WeakRef<object> {
  constructor(t: object) {
    super(t);
  }
  hi() {
    return "hi";
  }
}
const target = {};
const r = new Ref(target);
console.log(r.deref() === target, r.hi(), r instanceof WeakRef, r instanceof Ref);
console.log(Object.prototype.toString.call(r), Object.getPrototypeOf(Ref.prototype) === WeakRef.prototype);
try {
  new Ref(1 as any);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}

class Reg extends FinalizationRegistry<string> {
  count() {
    return "reg";
  }
}
const reg = new Reg(() => {});
const token = {};
console.log(reg.count(), reg instanceof FinalizationRegistry, reg instanceof Reg);
console.log(reg.register(target, "held", token), reg.unregister(token), reg.unregister(token));
try {
  new Reg(1 as any);
} catch (e: any) {
  console.log(e.constructor.name, e.message);
}
try {
  new (Reg as any)();
} catch (e: any) {
  console.log(e.constructor.name);
}

// The same check path for a Promise subclass executor.
class P extends Promise<number> {
  constructor(e: any) {
    super(e);
  }
}
try {
  new P(1);
} catch (e: any) {
  console.log(e.constructor.name);
}
