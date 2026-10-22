// An executor's `resolve` for a `Promise<number>` takes a number (or a
// thenable); stored into a slot typed `(v: number) => void`, it is
// called with a number later and settles the promise then.

let ok: (v: number) => void = (_v: number) => {};
const p = new Promise<number>((r) => {
  ok = r;
});
p.then((v) => console.log("settled", typeof v, v));
ok(5);

let late: (v: string) => void = (_v: string) => {};
const q: Promise<string> = new Promise((res) => {
  late = res;
});
q.then((v) => console.log("late", typeof v, v.length));
late("abc");
