// A top-level `const X: any = <literal>` is an Any slot (the annotation
// wins), so a read of it must load the box, not fold the literal: the
// folded `5` reached the reader as a bare integer that its Any-typed
// consumer refcounted. `let` spellings never folded, so only `const`
// crashed, and only through a named function.
const N: any = 5;
const F: any = 2.5;
const B: any = true;
const S: any = "s";
function rd(): any { return N }
function rf(): any { return F }
function rb(): any { return B }
function rs(): any { return S }
function sum(): any { return N + F }
console.log(rd(), rf(), rb(), rs(), sum());
console.log(N, F, B, S, N + 1, typeof N, typeof B);
const arr: any[] = [N, F, B];
console.log(arr.length, arr[0] + arr[1], arr[2]);
class C { v = 1 }
const K: any = C;
function mk(): any { return K }
console.log(rd(), mk() === C, new (mk())().v);
