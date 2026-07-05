// RFC 20260705 owned-result invariant: `return <call>` no longer
// force-moves idents inside the call (the result owns its own ref;
// receiver/args keep their normal scope drops).
function retChain(): number[] {
  let z = [7, 8, 9];
  return z.reverse();
}
console.log(retChain()[0]);

function retStrChain(): string[] {
  let z = ["m", "k"];
  return z.sort();
}
let r = retStrChain();
console.log(r[0]);
console.log(r[1]);

function passThrough(x: number[]): number[] {
  return x;
}
function retCallOnParam(x: string[]): string[] {
  return x.reverse();
}
let src = [1, 2, 3];
console.log(passThrough(src)[0]);
let ss = ["u", "v"];
let rr = retCallOnParam(ss);
console.log(rr[0]);
console.log(ss[0]);

function retNested(): number[] {
  let inner = [4, 5];
  return passThrough(inner);
}
console.log(retNested()[1]);
