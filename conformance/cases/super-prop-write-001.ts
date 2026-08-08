// SuperProperty write (§9.1.9 OrdinarySet, receiver = this) and the
// remaining read shapes: method-as-value, element form, statics.
class P {
  w = 0;
  m(): string {
    return "pm";
  }
  set s(x: number) {
    console.log("P.setter got", x);
  }
}
class Q extends P {
  m(): string {
    return "qm";
  }
  useElem(): any {
    return super["m"];
  }
  readMethod(): string {
    const f: any = super.m;
    return typeof f;
  }
  writeSetter() {
    super.s = 7;
  }
  writeData(): number {
    super.w = 5;
    return this.w;
  }
}
const q = new Q();
console.log(q.readMethod());
q.writeSetter();
const f2: any = q.useElem();
console.log(typeof f2);
console.log(q.writeData());

// Static context: super base is the parent CLASS OBJECT.
class SB {
  static sv = "psv";
}
class SD extends SB {
  static probe(): any {
    return super.sv;
  }
}
console.log(SD.probe());
