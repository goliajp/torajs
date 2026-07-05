// toSorted on refcounted (str) elements: the clone must own its element
// refs (arr_slice memcpys slots without inc). Inner-scope clone drop must
// not steal the source's refs — source stays fully readable after.
let a = ["banana", "apple", "cherry"];
{
  let b = a.toSorted();
  console.log(b[0]);
  console.log(b[2]);
}
console.log(a[0]);
console.log(a[2]);
// fresh (heap) strings via concat — clean per-elem accounting path
let s1 = "zz" + 7;
let s2 = "aa" + 3;
let c = [s1, s2];
let d = c.toSorted();
console.log(d[0]);
console.log(d[1]);
console.log(c[0]);
console.log(c[1]);
