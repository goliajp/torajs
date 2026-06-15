// W-J Phase D — nested named-class struct cell via Any binding.
// `const c: any` routes through `__torajs_print_anyv` Tag::Obj arm,
// which calls __torajs_anyv_struct_print_inline. The field walk
// recurses through __torajs_print_anyv_inline (nested-print
// substrate trunk Commit 1), so a nested struct field would walk
// the same path. This fixture verifies the Any → Tag::Obj routing
// pathway lines up with the direct-typed-receiver path (same output
// as check-print-class-named-001).
class Box {
  v: number;
  tag: string;

  constructor(v: number, tag: string) {
    this.v = v;
    this.tag = tag;
  }
}

const b: any = new Box(42, "boxed");
console.log(b);
