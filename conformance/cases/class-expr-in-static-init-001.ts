// K6 residue — a class expression in a static field initializer or a
// computed key of a class written inside a function evaluates when
// that initializer runs, with the function's locals in reach, once
// per evaluation of the outer class.
const log: string[] = [];
function outer(k: number): any {
  log.push("outer " + k);
  class Holder {
    static tag = "H" + k;
    static Inner = class {
      v(): number { return k * 10; }
    };
    static Named = class Nm {
      who(): string { return Nm.name; }
    };
    static Sub = class extends Holder.Inner {
      v(): number { return super.v() + 1; }
    };
    static made = new Holder.Inner().v();
    static once = new (class { t = k + 100; })().t;
    static Keyed = class {
      static ["key_" + k] = k * 3;
    };
  }
  return Holder;
}
const H1 = outer(1);
const H2 = outer(2);
console.log(H1.made, H2.made, H1.Inner === H2.Inner, new H2.Inner().v());
console.log(H1.Inner.name, H1.Named.name, new H2.Named().who(), new H2.Sub().v(), H2.once);
console.log(Object.keys(H2.Keyed).join(","), H2.Keyed.key_2, Object.keys(H1.Keyed).join(","));
function keyed(tag: string): any {
  class K {
    static [((): string => { log.push("key " + tag); return "m_" + tag; })()] = 1;
  }
  return K;
}
const Ka = keyed("a");
console.log(Object.keys(Ka).join(","), log.join("|"));
// a top-level class keeps its behaviour
class Top {
  static I = class { v(): number { return 5; } };
}
console.log(new Top.I().v(), Top.I.name);
