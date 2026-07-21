// RFC 20260721-array-proto-cluster 刀 13d — reverse on an
// accessor/exotic receiver rides the §23.1.3.26 four-step MOP pair
// loop (HasProperty / Get / Set / DeletePropertyOrThrow in exact
// low→high access order); a getter shrinking the array mid-loop
// turns the vacated lower index into a delete, and accessor
// getter/setter invocations are observable in spec order.

// get_if_present_with_delete shape: the getter empties the array, so
// lower no longer exists → Set(upper, lowerValue) + Delete(lower)
{
  const array = ["first", "second"];
  Object.defineProperty(array, 0, {
    get: function () {
      array.length = 0;
      return "first";
    },
  });
  array.reverse();
  console.log(0 in array, 1 in array, array[1]);
}
// low→high access order over accessor pairs
{
  const observed: string[] = [];
  const arr = ["a", "b", "c", "d"];
  Object.defineProperty(arr, "0", {
    configurable: true,
    get() {
      observed.push("0g");
      return "v0";
    },
    set(v) {
      observed.push("0s:" + v);
    },
  });
  Object.defineProperty(arr, "3", {
    configurable: true,
    get() {
      observed.push("3g");
      return "v3";
    },
    set(v) {
      observed.push("3s:" + v);
    },
  });
  arr.reverse();
  console.log(observed.join(","));
  console.log(arr[1], arr[2]);
}
// clean receiver keeps the raw-swap fast path
{
  const a = [1, 2, 3];
  console.log(a.reverse(), a[0]);
}
