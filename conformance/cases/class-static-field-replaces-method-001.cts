// §15.7.14: every static method is defined before any static field
// initializer runs, and a static field's DefineField replaces a
// same-named method only once its own initializer has been evaluated
// — so inside the initializers up to and including the field's, the
// name is still the method; after that, and everywhere else, the
// field (test262 class/elements/static-field-redeclaration).
class C {
  static f = 'test';
  static f = this.f + '262';
  static g() { return 45; }
  static g = this.g();
  static k = typeof this.h;
  static h = 1;
  static h2 = typeof this.h;
  static h() { return 2; }
  static { this.seen = typeof this.m; }
  static m() { return 'm'; }
  static m = 7;
}
console.log(C.f, C.g, C.k, C.h, C.h2, C.seen, C.m, typeof C.g);
console.log(Object.keys(C).join(","));
