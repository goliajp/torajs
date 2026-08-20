//! Closure-face dispatch primitives extracted from
//! [`crate::method_call`] to keep that god-file under the 500-line
//! project cap.
//!
//! The shared shape is the **uniform boxed-adapter ABI**
//! `(env: *mut c_void, argv: *const u64, argc: i64) -> AnyValue`:
//! every closure body a caller might reach through an `any`-typed
//! reference is compiled with a boxed dual entry at
//! `env + CLOSURE_BOXED_ENTRY_OFF` that follows this signature, so
//! dispatch is a single indirect call regardless of the closure's
//! declared arity or param types. `MAX_BOXED_ARGS` fixes the buffer
//! size the adapter reads (padded with `VALUE_UNDEFINED`); wider
//! calls take a heap-sized argv per RFC 20260708 to avoid the
//! silent-truncate the fixed buffer would otherwise imply.
//!
//! Two extern entry points expose this to the SSA lower:
//! - [`__torajs_any_call`] — `f(args…)` where the callee itself is an
//!   `any`. A non-closure receiver / missing adapter is a catchable
//!   TypeError via [`crate::method_call::not_callable`].
//! - [`__torajs_closure_call_variadic`] — raw-pointer twin used when
//!   the callee is Closure-typed but the call site is a variadic
//!   `(...args: E[]) => R` annotation (RFC 20260708-variadic).
//!
//! Callers outside this module import through
//! [`crate::method_call`] via the `pub(crate) use` re-exports there;
//! no consumer needs to know about this sibling by name.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::method_call::not_callable;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell};

/// Closure-cell boxed dual-entry offset — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_BOXED_ENTRY_OFF`.
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;

/// The boxed adapters read up to 8 param slots unconditionally —
/// mirror of torajs-core `ssa_lower_boxed_entry::MAX_BOXED_PARAMS`.
pub(crate) const MAX_BOXED_ARGS: usize = 8;

/// Resolve a callback-shaped AnyValue to its `(closure cell, boxed
/// dual entry)` pair — `None` for anything that can't dispatch
/// through the uniform ABI (non-cell, non-closure, entry 0).
pub(crate) unsafe fn closure_boxed_entry(av: AnyValue) -> Option<(*mut c_void, u64)> {
    if !is_cell(av) {
        return None;
    }
    unsafe { closure_cell_entry(as_void_ptr(av)) }
}

/// Raw-pointer variant of [`closure_boxed_entry`] for slots that
/// hold the env cell directly (a `Closure`-typed struct field).
pub(crate) unsafe fn closure_cell_entry(ptr: *mut c_void) -> Option<(*mut c_void, u64)> {
    unsafe {
        if ptr.is_null() || (ptr.cast::<u8>().add(4) as *const u16).read() != Tag::Closure as u16 {
            return None;
        }
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == 0 {
            return None;
        }
        Some((ptr, entry))
    }
}

/// Invoke a boxed dual entry through the uniform
/// `(env, argv, argc) -> AnyValue` ABI. argv rides in a fixed
/// 8-slot undefined-filled buffer so the adapter reads its param
/// count unconditionally; the return is caller-owned per the
/// boxed-value convention.
pub(crate) unsafe fn invoke_boxed(
    env: *mut c_void,
    entry: u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let n = argc.max(0) as usize;
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            core::mem::transmute(entry as usize);
        // RFC 20260708-closure-argv-face — an argv-face body reads
        // ALL argc slots off the pointer it receives (the
        // `__torajs_arguments` materializer), so a beyond-buf call
        // takes a heap-sized copy instead of silently truncating;
        // the common ≤ MAX_BOXED_ARGS shape stays on the stack.
        if n > MAX_BOXED_ARGS {
            let mut big = vec![VALUE_UNDEFINED; n];
            for (i, slot) in big.iter_mut().enumerate() {
                *slot = *argv.add(i);
            }
            return call(env, big.as_ptr(), argc);
        }
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        for (i, slot) in buf.iter_mut().enumerate().take(n) {
            *slot = *argv.add(i);
        }
        call(env, buf.as_ptr(), argc)
    }
}

/// [`invoke_boxed`] with the receiver prepended in argv[0] — the
/// dispatch shape for a `FLAG_CLOSURE_RECV_FIRST` closure stored as
/// an own property (an any-lane object-literal method whose body
/// says `this`; RFC 20260717-objlit-anylane-recv knife 1). The
/// promoted body's first declared param is `__this: any`, so the
/// adapter maps argv[0] onto it and the user args shift up by one.
pub(crate) unsafe fn invoke_boxed_recv_first(
    env: *mut c_void,
    entry: u64,
    recv: u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        let n = argc.max(0) as usize;
        let call: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 =
            core::mem::transmute(entry as usize);
        if n + 1 > MAX_BOXED_ARGS {
            let mut big = vec![VALUE_UNDEFINED; n + 1];
            big[0] = recv;
            for (i, slot) in big.iter_mut().skip(1).enumerate() {
                *slot = *argv.add(i);
            }
            return call(env, big.as_ptr(), argc + 1);
        }
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        buf[0] = recv;
        for (i, slot) in buf.iter_mut().skip(1).enumerate().take(n) {
            *slot = *argv.add(i);
        }
        call(env, buf.as_ptr(), argc + 1)
    }
}

/// `f(args…)` where the callee itself is an `any` value (RFC C4+
/// bare any-call). A `Tag::Closure` cell with a non-zero boxed dual
/// entry invokes through the uniform ABI; every other shape —
/// primitives, non-closure cells, closures without an adapter — is
/// a catchable TypeError. argv slots are BORROWED (the lowerer
/// rc-decs the boxes it made after the call).
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_call(
    recv: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_boxed_entry(recv) {
            return invoke_with_this(env, entry, VALUE_UNDEFINED, argv, argc);
        }
        not_callable()
    }
}

/// The explicit-`this` twin of [`__torajs_any_call`] — a dyn kernel
/// that owes its callback a real thisArg (`Array.fromAsync`'s
/// proposal §2.1.1 step 3.j.ii.6.a is `Call(mapfn, thisArg,
/// «value, k»)`) routes here; [`invoke_with_this`] owns the
/// recv-first shift exactly as the plain lane does, so a non-`this`
/// body drops the receiver per §10.2.1.2. `this_arg` is a borrow
/// like the argv slots.
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call; `this_arg` stays alive across the call too.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_call_with_this(
    recv: AnyValue,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_boxed_entry(recv) {
            return invoke_with_this(env, entry, this_arg, argv, argc);
        }
        not_callable()
    }
}

/// §6.2.6.5 steps 7-8 IsCallable over an Any-typed accessor face —
/// `defineProperty(o, k, {get: g})` where `g`'s static type erased
/// to `any`. A closure cell answers its pointer with a fresh stake
/// (the AccessorPair takes faces by ownership transfer); undefined
/// / null clear the face (NULL); anything else records the
/// catchable TypeError and answers NULL for the caller's throw
/// check. Pre-fix the face slot stored the NaN-box BITS verbatim
/// and the property access transmuted them as a cell — SIGSEGV.
///
/// # Safety
/// `v` is a valid AnyValue; a Heap payload points at a live cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_accessor_face_from_any(v: AnyValue) -> *mut c_void {
    unsafe {
        if crate::nanbox::is_undefined(v) || crate::nanbox::is_null(v) {
            return core::ptr::null_mut();
        }
        if is_cell(v) {
            let p = as_void_ptr(v);
            if (p.cast::<u8>().add(4) as *const u16).read() == Tag::Closure as u16 {
                crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
                return p;
            }
        }
        crate::method_call::not_callable();
        core::ptr::null_mut()
    }
}

/// Invoke honoring the closure's receiver channel. A receiver-first
/// closure (RFC 20260717-objlit-anylane-recv: an any-lane literal
/// method whose body says `this`) declares `__this` as its first
/// param, so the plain boxed ABI would feed it argv[0] — a detached
/// `g(5)` silently bound `this = 5` and dropped the arg, and
/// `f.call(other)` dropped `other` entirely. Flags bit 12 prepends
/// `this_arg`; a plain closure takes the direct boxed path (its
/// thisArg drops, matching a non-`this` body). Detached call sites
/// pass `VALUE_UNDEFINED` per §10.2.1.2 OrdinaryCallBindThis
/// (strict, no thisArgument); `.call` / `.apply` / bound cells pass
/// their real thisArg.
pub(crate) unsafe fn invoke_with_this(
    env: *mut c_void,
    entry: u64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        // S2.34 — a reified class-METHOD face carries its boxed
        // adapter in the cap slot and wants the RECEIVER in the env
        // slot (`adapter(this-as-env, argv, argc)`, the exact shape
        // `struct_method` dispatch uses). A non-cell thisValue falls
        // through to the bare entry's this-undefined TypeError — the
        // mono body reads the class layout off `this`, so a primitive
        // receiver can never run it. Accessor faces don't answer the
        // method-face probe and keep their own not-callable path.
        if let Some(adapter) = crate::method_value_class::class_method_face_adapter(env) {
            // RFC 20260804-fn-this-channel knife 3b — a STATIC method
            // face encodes as `(tag 0, twin ≠ 0)`: its mono body has
            // no receiver channel (a static `this` resolved to the
            // class object at compile time), so a rebind always takes
            // the `__smany_` twin, whatever the thisValue — instance,
            // class object, foreign object, or primitive (§27.7.5.1
            // OrdinaryCallBindThis puts no shape bound on it).
            let (face_tag, twin) = crate::method_value_class::class_method_face_guard(env);
            if face_tag == 0 && twin != 0 {
                return invoke_boxed_recv_first(env, twin, this_arg, argv, argc);
            }
            if is_cell(this_arg) {
                // Blade 3 (RFC 20260804-method-rebind-generic-body)
                // — the mono body reads the receiver at baked class
                // offsets, sound only when the receiver really is
                // this class. Guard on the instance's class tag
                // (precise: a subclass fails too — its layout may
                // extend); a foreign receiver routes to the
                // receiver-polymorphic `__cmany_` twin, whose body
                // reads members through the any lane. tag 0 disarms
                // (runtime-define mint sites); twin 0 = no twin
                // minted (this-free never guards here; super-route
                // bodies are the recorded residue) — keep the mono
                // path, today's behavior.
                let recv = as_void_ptr(this_arg);
                if face_tag != 0 && twin != 0 && !receiver_is_class(recv, face_tag) {
                    return invoke_boxed_recv_first(env, twin, this_arg, argv, argc);
                }
                return invoke_boxed(recv, adapter, argv, argc);
            }
            // S2.38 — a compiler-proven receiver-free body runs a
            // bare / primitive-`this` call with a null receiver (ES
            // §10.2.1.2: a this-free body runs regardless of the
            // thisArgument). Receiver-reading bodies keep the bare
            // entry's TypeError below — the mono body reads the
            // class layout off `this`, so a primitive can never
            // safely run those.
            let flags = (env as *const u8).add(6).cast::<u16>().read();
            if flags & torajs_rc::FLAG_CLASS_METHOD_THIS_FREE != 0 {
                return invoke_boxed(core::ptr::null_mut(), adapter, argv, argc);
            }
            // RFC 20260820-member-call-route knife 3 — every other
            // thisValue (int / f64 / bool immediates, and nullish)
            // hosts the receiver-polymorphic `__cmany_` twin:
            // §10.2.1.2 OrdinaryCallBindThis binds the thisArgument
            // as-is in a strict (class) body, and the spec TypeError
            // belongs to the member READ/WRITE site, not the call
            // boundary — the twin's any-lane kernels raise it there
            // (§13.2.3 GetValue on nullish, the §7.3.31/.32 brand
            // check on a brand-less receiver), so a body-side `try`
            // observes it and pre-throw side effects run (the
            // privatefieldget/put-primitive-receiver family asserts
            // exactly this). A twin-less mono body keeps the
            // bare-entry TypeError below: it reads the class layout
            // off `this` and can host nothing foreign.
            if face_tag != 0 && twin != 0 {
                return invoke_boxed_recv_first(env, twin, this_arg, argv, argc);
            }
        }
        if recv_first_shift(env) != 0 {
            return invoke_boxed_recv_first(env, entry, this_arg, argv, argc);
        }
        invoke_boxed(env, entry, argv, argc)
    }
}

/// Blade 3 — `true` when `recv` is a `Tag::Obj` instance whose
/// baked `class_tag@+8` equals the face's owning class (mirror of
/// torajs-core `ssa_lower::OBJ_CLASS_TAG_OFF`). Precise equality:
/// a subclass instance answers `false` and takes the twin's
/// any-lane reads (its layout may extend the parent's).
///
/// # Safety
/// `recv` is a live heap cell pointer.
unsafe fn receiver_is_class(recv: *mut c_void, face_tag: u64) -> bool {
    unsafe {
        (recv.cast::<u8>().add(4) as *const u16).read() == Tag::Obj as u16
            && u64::from(recv.cast::<u8>().add(8).cast::<u32>().read()) == face_tag
    }
}

/// 1 when the closure cell declares the receiver-first channel
/// (flags bit 12), else 0 — the argv slot shift a HOF loop applies
/// so its `(v, k, O)` triple lands on the USER params while the
/// `__this` slot reads the buffer's `undefined` padding (per spec
/// every no-thisArg callback binds `this = undefined`). Hoisted out
/// of the loop by callers: one flags read per walk, zero per-iter
/// cost for plain callbacks.
///
/// # Safety
/// `env` is a live `Tag::Closure` cell.
pub(crate) unsafe fn recv_first_shift(env: *mut c_void) -> usize {
    unsafe {
        let flags = (env as *const u8).add(6).cast::<u16>().read();
        usize::from(flags & torajs_rc::FLAG_CLOSURE_RECV_FIRST != 0)
    }
}

/// `f(args…)` where `f` is a Closure-typed slot called through a
/// variadic fn-type annotation (`(...args: E[]) => R`) — the
/// raw-pointer twin of [`__torajs_any_call`] (RFC 20260708-variadic:
/// one h body serves closures of any declared arity, so the call
/// dispatches through the boxed dual entry instead of a static
/// declared-pair ABI). A missing adapter (>8 params / unboxable
/// face) is the same catchable TypeError the any-call lane answers.
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call; `env` is a (possibly null) closure env cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_call_variadic(
    env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_cell_entry(env) {
            return invoke_with_this(env, entry, VALUE_UNDEFINED, argv, argc);
        }
        not_callable()
    }
}

/// The explicit-`this` twin of [`__torajs_closure_call_variadic`] —
/// 398-06's typed-lane recv arm: a call site whose static promotion
/// answer was "no" found `FLAG_CLOSURE_RECV_FIRST` set at runtime and
/// re-routes here with the receiver it owes the callee (the §13.3.6.2
/// method receiver, a `.call`/`.apply` thisArg, or the §10.2.1.2
/// strict `undefined`). [`invoke_with_this`] owns the shift/dispatch
/// story exactly as the any lane always has.
///
/// # Safety
/// `argv` points at `argc` AnyValue slots the caller keeps alive
/// across the call; `env` is a (possibly null) closure env cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_call_with_this(
    env: *mut c_void,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        if let Some((env, entry)) = closure_cell_entry(env) {
            return invoke_with_this(env, entry, this_arg, argv, argc);
        }
        not_callable()
    }
}
