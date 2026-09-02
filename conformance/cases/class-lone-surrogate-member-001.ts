// 557-02 C 组 — class member names are property keys (UTF-16 code-unit
// sequences), so a lone surrogate names a member of its own and never
// collapses into U+FFFD. The desugared symbol spells it through an
// escape (`__u_d800`) that round-trips, and a key literally spelled
// like the escape stays distinct.
class A {
  "\uD800"() {
    return "hi";
  }
  "__u_d800"() {
    return "literal";
  }
  get "\uDC00"() {
    return 42;
  }
  ["\uD800x"]() {
    return "computed";
  }
  static "\uD800" = 3;
  static "\uDBFF"() {
    return "static";
  }
  "\uDC01" = 7;
}

const a: any = new A();
console.log(a["\uD800"](), a["__u_d800"](), a["\uDC00"], a["\uD800x"](), a["\uDC01"]);
const K: any = A;
console.log(K["\uD800"], K["\uDBFF"]());
// Sorted: own-property ORDER on a prototype / class object is a
// separate, pre-existing gap (562-01); this fixture is about the keys.
console.log(JSON.stringify(Object.getOwnPropertyNames(A.prototype).sort()));
console.log(JSON.stringify(Object.getOwnPropertyNames(A).sort()));
console.log(JSON.stringify(a["\uD800"].name), a["\uD800"].name.length, a["\uD800"].name.charCodeAt(0));
console.log(JSON.stringify(K["\uDBFF"].name));
const d = Object.getOwnPropertyDescriptor(A.prototype, "\uDC00");
console.log(typeof d?.get, JSON.stringify(d?.get?.name));
