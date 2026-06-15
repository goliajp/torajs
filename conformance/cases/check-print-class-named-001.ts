// W-J Phase D — Tag::Obj struct cell pretty print, named class
// with multi-type fields. inspect.rs Tag::Obj arm calls
// __torajs_anyv_struct_print_inline (struct_print.rs) which reads
// class_tag@+8 → __torajs_struct_class_name(tag) for the `Point {`
// prefix → __torajs_struct_field_{count,name,info} walks declared
// fields in order → field_slot_to_pair + anyv_box_from_pair feeds
// each value through __torajs_print_anyv_inline. Closes the W-J
// Phase D `[object]` fallback wedge for named-class instances.
class Point {
  x: number;
  y: number;
  label: string;
  active: boolean;

  constructor(x: number, y: number, label: string, active: boolean) {
    this.x = x;
    this.y = y;
    this.label = label;
    this.active = active;
  }
}

const p = new Point(3, 7, "origin", true);
console.log(p);
