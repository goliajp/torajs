// Any-method-call RFC 20260704 C1 — push/pop on any-array receivers,
// charAt/toUpperCase/toLowerCase on any-string receivers, throw faces.

// 1. Arr<Any> literal behind any — variadic push, ES return value
//    (new length), growth keeps the receiver current (top-level
//    global slot write-back).
const a: any = [1, 2];
console.log(a.push(3));
console.log(a.push(4, 5));
console.log(a[4]);
console.log(a.length);

// 2. typed number[] moved into any — kind-matched push (raw i64
//    slots) + grow relocation chased through the local slot.
function grown(): any {
  const t: number[] = [1, 2];
  const b: any = t;
  b.push(3);
  b.push(4, 5);
  b.push(6);
  b.push(7);
  console.log(b[6]);
  console.log(b.length);
  return b;
}
const b: any = grown();

// 3. pop — value out, length shrinks; empty answers undefined.
console.log(b.pop());
console.log(b.length);
const c: any = ["pop-heap-string-aaaaaaaaaaaaaaaa"];
console.log(c.pop());
console.log(c.pop());

// 4. string methods — heap Str and ShortStr receivers.
const s: any = "hello world";
console.log(s.charAt(0));
console.log(s.charAt(4));
console.log(s.charAt(99));
console.log(s.toUpperCase());
const ss: any = "hi";
console.log(ss.toUpperCase());
console.log(ss.charAt(1));
const lo: any = "MiXeD";
console.log(lo.toLowerCase());

// 5. throw faces — null receiver and unknown method, both
//    catchable. (Kind-mismatch push — `numberArr.push("x")` behind
//    any — is tr's documented elem-kind boundary: catchable
//    TypeError where bun accepts, same S3-set non-parity; not in
//    this bun-compared fixture, see RFC 20260704.)
const n: any = null;
try {
  n.push(1);
} catch (e) {
  console.log("caught null");
}
try {
  a.frobnicate();
} catch (e) {
  console.log("caught unknown");
}

console.log("done");
