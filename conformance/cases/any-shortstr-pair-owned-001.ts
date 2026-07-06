// pair-unbox owned fuse — owning (tag, value) consumers take the slot's
// stake through __torajs_anyv_unbox_value_owned instead of a separate
// payload inc (chunk 610). Behavior must stay byte-equal with bun; the
// leak itself is covered by the AOT RSS probe (leak-pair-owned.ts,
// mini /tmp/rc4-dump).
const s: any = "ab";
const o: any = { k: s };
console.log(o.k);
const arr: any[] = [s, 1];
arr.push(s);
arr[1] = s;
console.log(arr[0], arr[1], arr[2], arr.length);
const m = new Map<string, any>();
m.set("k", s);
console.log(m.get("k"));
try {
  throw s;
} catch (e) {
  console.log("caught", e);
}
o.k2 = s;
console.log(o.k2);
const big: any = "abcdefgh";
const o2: any = { k: big, k2: s };
console.log(o2.k, o2.k2, big);
const arr2: any[] = [big];
arr2.push(big);
console.log(arr2[0], arr2[1]);
