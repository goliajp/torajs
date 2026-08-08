// species key 2 second cut — the create-species.js assert shape:
// `Object.getPrototypeOf(thisValue) === Ctor.prototype` is a member
// READ of the stored fn binding. Two guards used to reject it:
// the arguments kill walk counted the read as an illegal use (the
// argv face never materialized — runtime ReferenceError), and the
// knife-2 argv-contention bar treated any mixed use as a call-lane
// rider (the promote never happened — `__this` unbound). A
// `.prototype` read enters no call lane: it answers the fnprops
// canonical pair, and a round trip back to the fn value dispatches
// any-lane through the boxed dual entry. Both guards now exempt it.
var thisValue: any, args: any, result: any;
var callCount = 0;
var instance: any = [];
var Ctor = function () {
  callCount += 1;
  thisValue = this;
  args = arguments;
  return instance;
};
var a = [1, 2, 3, 4, 5];
a.constructor = {};
a.constructor[Symbol.species] = Ctor;
result = a.map(function () {});
console.log(callCount, args.length, args[0], result === instance);
console.log(Object.getPrototypeOf(thisValue) === Ctor.prototype);
