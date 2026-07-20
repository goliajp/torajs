// wrapper expando storing reified builtin method cells — invoking
// the stored cell runs the method BODY against the wrapper receiver
// (no second own-property resolve: a same-mid entry must not
// re-resolve to itself — the S15.6.4.2_A2 stack-overflow family).
const src: any = "xy";
const s1: any = new String("ab");
s1.toString = src.toString;
console.log(s1.toString());
const s2: any = new String("hello");
s2.slice = src.slice;
console.log(s2.slice(1));
console.log(s2.slice.call(s2, 2));
const s3: any = new String("q");
s3.foo = src.toString;
console.log(s3.foo());
