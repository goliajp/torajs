// Integration: string concatenation through a chain of `+`s.
function check(): number {
  let s: string = "Hello";
  let t: string = s + ", " + "world" + "!";
  if (t !== "Hello, world!") { throw "#1"; }
  let u: string = "" + "a" + "b" + "c" + "d";
  if (u !== "abcd") { throw "#2"; }
  if (u.length !== 4) { throw "#3"; }
  return 0;
}
console.log(check());
