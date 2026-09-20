// A generic class carries a computed-key static field: the field is
// the class object's, not the monomorphised twin's, and the twin the
// body cloner mints for the second instantiation must not lose it
// (RFC 20260921 records the clone as a second owner of the sentinel).
const k = "kk";
class G<T> { static [k] = 7; v: T; constructor(v: T) { this.v = v; } m(): T { return this.v; } }
const a = new G<number>(1), b = new G<string>("s");
console.log("generic", a.m(), b.m(), (G as any)[k]);
class H<T> { v: T; constructor(v: T) { this.v = v; } id(): T { return this.v; } }
console.log("twin", new H<number>(3).id(), new H<string>("x").id());
