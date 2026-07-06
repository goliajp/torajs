// RFC 20260707 chunk 623 — dropping a typed array through its
// static Arr<Any> owner routes per elem kind instead of the NaN-box
// slot walk. Pre-fix: a raw i64 slot with bit 1 clear (4, 8, 16)
// passed the cell predicate and was deref'd + freed (SIGSEGV when
// the any view held the last reference).
class Box {
  arr: any[] = [];
}

// owned transfer: the field holds the only reference, so the
// arr_drop_any walk is the block's last dropper
function make(): number[] {
  return [4, 8, 16];
}
function f1(): void {
  const b = new Box();
  b.arr = make();
}
f1();

// scope shape: the typed binding dies first, the any view second
function f2(): void {
  const b = new Box();
  {
    const nums: number[] = [4, 8, 16];
    b.arr = nums;
  }
}
f2();

// heap elems: the kind-routed drop walks children, so the strings
// release (leak-checked by the AOT probe, crash-checked here)
function makeStrs(): string[] {
  return ["aaaa-bbbb-cccc", "dddd-eeee-ffff"];
}
function f3(): void {
  const b = new Box();
  b.arr = makeStrs();
}
for (let i = 0; i < 1000; i++) f3();

console.log("ok");
