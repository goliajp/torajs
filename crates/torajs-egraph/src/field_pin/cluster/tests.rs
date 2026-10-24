use super::*;
use torajs_core::ssa::{ArrId, Block, BlockId, Inst, StructId, Terminator, ValueInfo};

const OFF: u64 = 32;

fn inst(result: Option<u32>, kind: InstKind) -> Inst {
    Inst {
        result: result.map(ValueId),
        kind,
        origin: None,
    }
}

fn v(i: u32) -> Operand {
    Operand::Value(ValueId(i))
}

/// A member `name`: %1 = this.f · %2 = its length · %3 = its data ·
/// then `calls` (each passing `%0`, or `%4`, another cell, when the
/// flag says so) · ret %2.
fn member(name: &str, calls: &[(u32, bool)]) -> Function {
    let mut insts = vec![
        inst(Some(1), InstKind::Load(Type::Arr(ArrId(0)), v(0), OFF)),
        inst(Some(2), InstKind::Load(Type::I64, v(1), ARR_LEN_OFF)),
        inst(Some(3), InstKind::Load(Type::Ptr, v(1), ARR_DATA_PTR_OFF)),
        inst(Some(4), InstKind::Load(Type::Obj(StructId(0)), v(0), 40)),
    ];
    for &(t, other) in calls {
        let recv = if other { v(4) } else { v(0) };
        insts.push(inst(None, InstKind::Call(FuncId(t), vec![recv])));
    }
    let ty = |t| ValueInfo { ty: t, name: None };
    Function {
        name: name.into(),
        params: vec![ValueId(0)],
        ret: Type::I64,
        blocks: vec![Block {
            id: BlockId(0),
            insts,
            term: Terminator::Ret(Some(v(2))),
        }],
        values: vec![
            ty(Type::Obj(StructId(0))),
            ty(Type::Arr(ArrId(0))),
            ty(Type::I64),
            ty(Type::Ptr),
            ty(Type::Obj(StructId(0))),
        ],
        current_origin: None,
    }
}

/// `main` calls member 0, which makes `calls`.
fn module(calls: &[(u32, bool)]) -> Module {
    let mut main = Function::new("main", Type::Void);
    let p = main.add_param(Type::Obj(StructId(0)), "p");
    let b = main.add_block();
    main.append_void(b, InstKind::Call(FuncId(0), vec![Operand::Value(p)]));
    main.set_term(b, Terminator::Ret(None));
    Module {
        funcs: vec![member("__cm_P__a", calls), member("__cm_P__b", &[]), main],
        pins: vec![PinFact {
            sid: StructId(0),
            field_off: OFF,
            arr: ArrId(0),
            cluster: vec![FuncId(0), FuncId(1)],
            entries: vec![FuncId(0)],
            direct_only: true,
            quiet: vec![],
            scalars: vec![],
        }],
        ..Default::default()
    }
}

fn kinds(f: &Function) -> Vec<&InstKind> {
    f.blocks
        .iter()
        .flat_map(|b| &b.insts)
        .map(|i| &i.kind)
        .collect()
}

#[test]
fn an_entry_loads_for_the_body_its_callers_reach_and_members_read_the_registers() {
    let mut m = module(&[(1, false)]);
    let st = pin_clusters(&mut m);
    assert_eq!(
        (st.clusters, st.members, st.entries, st.loads),
        (1, 2, 1, 4)
    );
    // a's body is new, under a local name; a itself only calls it
    let body = FuncId(3);
    assert_eq!(m.funcs[3].name, ".L__cm_P__a.pin");
    assert_eq!(m.pin_regs.entries, vec![(FuncId(0), body, OFF)]);
    assert_eq!(m.pin_regs.inner, vec![body, FuncId(1)]);
    let entry = kinds(&m.funcs[0]);
    assert_eq!(entry[0], &InstKind::Call(body, vec![v(0)]));
    assert!(
        m.funcs[0]
            .blocks
            .iter()
            .any(|b| (b.insts.iter()).any(|i| i.kind == InstKind::ThrowSignal))
    );
    // b is reached only from a's body with its receiver: no entry
    for f in [&m.funcs[3], &m.funcs[1]] {
        let k = kinds(f);
        assert_eq!(k[0], &InstKind::Pinned(PinWord::Len));
        assert_eq!(k[1], &InstKind::Pinned(PinWord::Data));
        // the field load went with the two it fed; the other field stays
        let loads = k.iter().filter(|k| matches!(k, InstKind::Load(..))).count();
        assert_eq!(loads, 1);
        assert!(
            matches!(f.blocks[0].term, Terminator::Ret(Some(Operand::Value(r))) if r != ValueId(2))
        );
    }
    assert!(kinds(&m.funcs[3]).contains(&&InstKind::Call(FuncId(1), vec![v(0)])));
}

#[test]
fn a_call_with_another_receiver_goes_through_an_entry() {
    let mut m = module(&[(1, false), (1, true)]);
    let st = pin_clusters(&mut m);
    assert_eq!(st.entries, 2);
    let bodies: Vec<FuncId> = m.pin_regs.entries.iter().map(|e| e.1).collect();
    assert_eq!(bodies, vec![FuncId(3), FuncId(4)]);
    // a's body calls b by its entry, which loads the other cell's words
    assert!(kinds(&m.funcs[3]).contains(&&InstKind::Call(FuncId(1), vec![v(4)])));
}

#[test]
fn a_copied_word_leaves_the_cluster_alone() {
    let mut m = module(&[(1, false)]);
    let f = &mut m.funcs[1];
    f.values.push(ValueInfo {
        ty: Type::I64,
        name: None,
    });
    f.blocks[0]
        .insts
        .push(inst(Some(5), InstKind::Copy(Type::I64, v(2))));
    let before = m.funcs.clone();
    let st = pin_clusters(&mut m);
    assert_eq!(st.clusters, 0);
    assert_eq!(m.funcs, before);
    assert_eq!(m.pin_regs, Default::default());
}

#[test]
fn members_that_never_call_each_other_are_left_alone() {
    let mut m = module(&[(1, true)]);
    let before = m.funcs.clone();
    assert_eq!(pin_clusters(&mut m).clusters, 0);
    assert_eq!(m.funcs, before);
}
