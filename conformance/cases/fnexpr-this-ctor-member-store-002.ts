// Same shape inside a function body, where the stored function also
// reads an outer local, plus a non-function property store and a
// plain member read of the binding.
function outer(a: number): number {
  const K: any = function (p: number) {
    this.x = p;
  };
  K.prototype.m = function (): number {
    return a + this.x;
  };
  K.s = function (): number {
    return a * 2;
  };
  K.tag = "k";
  return K.s() + new K(3).m() + K.tag.length;
}
console.log(outer(7));
