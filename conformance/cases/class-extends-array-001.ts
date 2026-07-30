// class C extends Array — exotic-backed instances (RFC 20260730
// blade 1). The instance is a REAL Tag::Arr cell (length magic /
// index storage / Array.isArray / instanceof Array for free); the
// class identity rides FLAG_SUBCLASSED + the blade-0 side table
// (instanceof C / getPrototypeOf / method dispatch).

// 1. default ctor, builtin faces
class MyArr extends Array {}
const m = new MyArr();
console.log(Array.isArray(m));
console.log(m instanceof Array);
console.log(m instanceof MyArr);
console.log(Object.getPrototypeOf(m) === MyArr.prototype);
console.log(m.length);
m.push(5);
console.log(m[0], m.length);

// 2. plain arrays keep their answers (and never read the side table)
const plain = [1, 2, 3];
console.log(plain instanceof MyArr);
console.log(Object.getPrototypeOf(plain) === Array.prototype);

// 3. explicit ctor with super(n) — new Array(n) length semantics
class Sized extends Array {
  constructor(n: number) {
    super(n);
  }
}
const s = new Sized(3);
console.log(s.length, s[0]);

// 4. class methods over the exotic receiver, builtins still riding
class Stack extends Array {
  peek(): any {
    return this[this.length - 1];
  }
}
const st = new Stack();
st.push(10);
st.push(20);
console.log(st.peek());
console.log(st.map((x: any) => x * 2));
console.log(JSON.stringify(st));
