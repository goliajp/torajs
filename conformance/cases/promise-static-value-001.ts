// Promise combinator statics read as VALUES (reflection + detached call)
console.log(typeof Promise.all, Promise.all.length, Promise.all.name);
console.log(typeof Promise.allSettled, Promise.allSettled.length, Promise.allSettled.name);
console.log(typeof Promise.any, Promise.any.length, Promise.any.name);
console.log(typeof Promise.race, Promise.race.length, Promise.race.name);

// detached-this call must raise a catchable TypeError (§27.2.4.1 step 1)
function ZeroArgConstructor() {}
try {
  Promise.all.call(ZeroArgConstructor, []);
  console.log("no throw");
} catch (e) {
  console.log("caught", e instanceof TypeError);
}

// aliased detached value, bare call — same TypeError
const detachedRace = Promise.race;
try {
  detachedRace([]);
  console.log("no throw 2");
} catch (e) {
  console.log("caught2", e instanceof TypeError);
}
