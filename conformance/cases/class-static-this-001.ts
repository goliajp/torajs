class C {
  static v = 3;
  static m() { return 10 }
  static c() { return this.m() + this.v }
}
console.log(C.c());
