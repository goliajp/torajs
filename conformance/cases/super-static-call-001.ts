// Static-context super: method calls dispatch to __sm_<owner>__<m>
// (no receiver param) with the owner resolved along the STATIC method
// lists; getters keep working through the Pass 1.7 read path.
class SB {
  static sm(): string {
    return "psm";
  }
  static get sg(): string {
    return "psg";
  }
}
class SM extends SB {}
class SD extends SM {
  static sm(): string {
    return "dsm";
  }
  static probe(): string {
    return super.sm();
  }
  static probeGetter(): string {
    return super.sg;
  }
}
console.log(SD.probe(), SD.probeGetter());
console.log(SD.sm());
