// A cast receiver is the array itself: `(c as any[]).push(v)` appends
// to `c`.
const c: number[] = [1, 2];
(c as any[]).push("s");
console.log(c.length, (c as any[])[2], typeof (c as any[])[2]);
const n: number[] = [3];
(n as number[]).push(4);
console.log(n.join(","));
const s: string[] = ["a"];
(s as any[]).push(7);
console.log(s.length, typeof (s as any[])[1]);
