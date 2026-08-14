// 398-08 regression pin — the mixed-scalar ternary written inside a
// class-nested fn-expr used to exit 139 with no output; closed as a
// side effect of the 398-07 knife (rotation 400).

class K {
  v = 5;
  f: any;
  constructor() {
    this.f = function () {
      return (this as any) === undefined ? "u" : (this as any).v;
    };
  }
}
const k = new K();
const d = k.f;
console.log(d());
