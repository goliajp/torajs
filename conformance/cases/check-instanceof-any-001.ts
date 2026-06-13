// `<v: any> instanceof <Class>` — runtime tag-dispatch path closes
// the Type::Any wedge in ssa_lower's static instanceof folding.
//
// Auto-injected TypeError / RangeError catches expose this: the
// catch binding is typed `:any`, so the inline `Type::Obj(_)` path
// can't see through the NaN-box. ssa_lower now emits one
// `__torajs_instanceof_class_any_tag` call per descendant tag,
// OR-chained — matching the Type::Obj path's class-tag shape but
// going through the runtime to unbox first.
//
// Coverage:
//   1. catch (e: any) instanceof native-error subclass / parent
//   2. user class hierarchy via :any binding (descendant_tag walk)
//   3. cross-class negative (TypeError tag != user-class tag)
//   4. non-heap NaN-box tags (Number / Str) return false safely

try {
  Object.defineProperty(null, "x", { value: 1 });
} catch (e) {
  console.log(e instanceof TypeError);
  console.log(e instanceof RangeError);
  console.log(e instanceof Error);
}

class Animal {
  name: string = "";
}
class Dog extends Animal {
  breed: string = "";
}
const d: any = new Dog();
console.log(d instanceof Dog);
console.log(d instanceof Animal);
console.log(d instanceof TypeError);

const n: any = 42;
console.log(n instanceof TypeError);

const s: any = "hi";
console.log(s instanceof Error);
