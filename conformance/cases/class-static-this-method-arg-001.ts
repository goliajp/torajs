function check<T>(a: T, b: T, msg: string = ""): number { return 2 }
const r: any = 3;
class C {
  static method() {
    return check(r, this.method);
  }
}
console.log(C.method());
