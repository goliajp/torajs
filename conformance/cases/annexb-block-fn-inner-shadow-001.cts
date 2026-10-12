// Annex B §B.3.3 splits a capturing block function into a block binding
// and a var binding, and renames the block's references to the block
// one. A block nested inside that declares the same name has its own
// binding (§14.2.2), and the rename must leave its references alone:
// they used to call the outer function and print 1 where node prints 9.
//
// bun is not an oracle here: it reads every file as strict code, where
// there is no var binding and the last call throws. node follows the
// spec, so this checks against a `.expected`.

function n() {
  var x = 1;
  {
    function h() { return x; }
    {
      const h = () => 9;
      console.log(h());
    }
    console.log(h());
  }
  return h();
}
console.log(n());
