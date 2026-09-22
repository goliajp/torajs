// A runtime kernel that runs ToPrimitive on an `any` operand INSIDE the
// runtime reaches the user object's valueOf / toString through the
// object dispatch arms; this program's only object-world entry is that
// kernel, so the dispatch judgment must keep the arms.
const q: any = { toString() { return "k"; }, valueOf() { return 2; } };
class S extends String {
  constructor(n: any) {
    super(n);
  }
}
console.log(new S(q).length);
