// P1 rc bug regression fixture (2026-07-23):
//   `const t = xs.values(); t.next(); const objs = [{id:1}]; for(o of objs.values()){}`
// crashed with SIGSEGV during the for-of exhaustion latch because the
// IteratorResult struct (`{value: any, done: boolean}`) added by
// `t.next()` at Pass-2 shifted anonymous class layout indices —
// `populate_class_layouts` emitted IteratorResult at the tag position
// the pool had reserved for `{id:1}`. Runtime cycle walker then read
// `{id:1}`'s scalar `id` field as a heap child pointer.
// Fix: populate walks struct_layouts only up to the Pass-1.5 snapshot
// boundary; every Pass-2 addition emits via append_fresh at the
// pool-assigned tag.
const inf: any[] = [10, 20, 30]
const t = inf.values()
t.next()
const objs: any[] = [{ id: 1 }, { id: 2 }]
console.log('before-objs-loop')
for (const o of objs.values()) {
  console.log('iter', (o as any).id)
}
console.log('after-objs-loop')
