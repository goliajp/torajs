// rotation 560 — every runtime read of a Str payload compares by the
// cell's WTF-8 spelling, never by `len` raw bytes. A UTF-16 key whose
// first `len` payload bytes happen to spell an ASCII name is not that
// name: "ㄱ" (U+3131) is not "1", "慮敭ab" is not "name", "楳敺ab" is
// not "size". A UTF-16 gap / rawJSON text / spread exclusion list is
// spelled whole, not cut in half.
const arr: any = [10, 20, 30];
console.log(arr["ㄱ"], arr["1"], arr["㸱"]);
arr["ㄱ"] = 99;
console.log(arr[1], arr["ㄱ"], arr.length);
Object.defineProperty(arr, "㸱", { value: 7, enumerable: true });
console.log(arr[1], arr["㸱"], Object.keys(arr).length);

const sw: any = new String("abc");
console.log(Object.getOwnPropertyDescriptor(sw, "㸱"), sw["㸱"]);
console.log(Object.getOwnPropertyDescriptor("abc", "㸱"));

const fn: any = function foo() {};
console.log(Object.getOwnPropertyDescriptor(fn, "慮敭ab"));
console.log(Object.getOwnPropertyDescriptor(fn, "name")?.value);

const m: any = new Map([[1, 2]]);
console.log(m["楳敺ab"], m.size);

console.log(JSON.stringify({ a: JSON.rawJSON('"😀"'), b: JSON.rawJSON('"é"') }));

console.log(JSON.stringify({ a: 1, b: [1] }, null, "→→"));
console.log(JSON.stringify({ a: 1 }, null, "éé"));
console.log(JSON.stringify({ a: 1 }, null, "中中中中中中中中中中中中"));
console.log(JSON.stringify({ a: 1 }, null, "😀😀😀😀😀😀"));
console.log(JSON.stringify({ a: 1 }, (k, v) => v, "→→"));

const o: any = { a: 1, 中: 2, 㸭: 3 };
const { 中, ...rest } = o;
console.log(JSON.stringify(rest), 中);
const o2: any = { a: 1, 中: 2, b: 3 };
const { a, 中: z, ...r2 } = o2;
console.log(JSON.stringify(r2), a, z);

class M extends Map<string, number> {
  取(k: string) {
    return this.get(k);
  }
}
const m2: any = new M();
m2.set("k", 7);
console.log(m2.取("k"));

const src: any = { 中: 1, 㸭: 2 };
const keys: string[] = [];
for (const k in src) keys.push(k);
console.log(keys.join(","));
