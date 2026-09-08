// A reified class-accessor face invoked as a method off a property
// descriptor. The METHOD-face receiver guard `(class_tag, twin)` and
// the ACCESSOR face's `(name, length)` occupy the same two words, so
// reading the guard blind made every setter face (length 1) answer
// "twin = 1" and the dynobj method-call arms jumped to address 1.
// Getters (length 0) read as "no twin" and always worked — hence the
// pair split. Bodies here never read `this`, so §10.2.9's own-name
// binding is all that runs.
class C {
  get g(): number { console.log("getter ran"); return 7; }
  set s(x: any) { console.log("setter ran", x); }
  static get sg(): number { console.log("static getter ran"); return 8; }
  static set ss(x: any) { console.log("static setter ran", x); }
}

const gd: any = Object.getOwnPropertyDescriptor(C.prototype, "g");
const sd: any = Object.getOwnPropertyDescriptor(C.prototype, "s");
console.log(typeof gd.get, gd.get.name, gd.get.length);
console.log(typeof sd.set, sd.set.name, sd.set.length);
console.log(gd.get());
sd.set();
sd.set(1);

// the same face under a name that is not "get" / "set" — the crash
// was never about the property name it was read back through.
const holder: any = {};
holder.zzz = sd.set;
holder.zzz(2);
holder.yyy = gd.get;
console.log(holder.yyy());

// static half of the pair
const sgd: any = Object.getOwnPropertyDescriptor(C, "sg");
const ssd: any = Object.getOwnPropertyDescriptor(C, "ss");
console.log(sgd.get());
ssd.set(3);

// and through the accessor pair's normal channel, unchanged
const inst = new C();
console.log(inst.g);
inst.s = 4;
