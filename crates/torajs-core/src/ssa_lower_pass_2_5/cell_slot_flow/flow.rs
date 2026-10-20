//! The scans of every function `cell_slot_flow` reads, added up into
//! the open sids and the prefix paths, and the two answers the pass
//! asks for at once: over every function, and over the functions that
//! run.

use std::cell::RefCell;

use crate::ssa::id_hash::{HashMap, HashSet};
use crate::ssa::{FuncId, Module, StructId, Type};

use super::super::any_holds::AnyHolds;
use super::{BOX_ALLOC, EnvLayouts, FITS, OBJ_ALLOC, Paths, Scan};

/// [`open_sids`] (`all`), and [`open_sids_with`] over the functions
/// neither `asleep` nor the dead adapters rule out (`skip`): the scans
/// the two share run once.
pub(crate) struct Both {
    pub all: Vec<bool>,
    pub open: Vec<bool>,
    pub widens: Vec<(StructId, StructId)>,
    pub skip: Vec<bool>,
}

pub(crate) fn open_sids_both(module: &Module, envs: &EnvLayouts, asleep: &[bool]) -> Both {
    let holds = AnyHolds::of(module);
    let skip: Vec<bool> = (asleep.iter().enumerate())
        .map(|(i, a)| *a || holds.is_dead(FuncId(i as u32)))
        .collect();
    let mut flow = Flow::new(module, envs, &holds);
    flow.scan(|i| !skip[i]);
    let (mut open, paths) = (flow.open.clone(), flow.paths.borrow().clone());
    flow.scan(|i| skip[i]);
    let all = flow.settle().0;
    let widens = paths.widens();
    paths.settle(&mut open);
    Both {
        all,
        open,
        widens,
        skip,
    }
}

/// What every function's scan reads, and the openness and the paths
/// the scans add up to.
pub(super) struct Flow<'a> {
    module: &'a Module,
    envs: &'a EnvLayouts,
    holds: &'a AnyHolds<'a>,
    globals: HashMap<&'a str, Type>,
    alloc: Option<usize>,
    box_alloc: Option<usize>,
    fits: Vec<FuncId>,
    open: Vec<bool>,
    paths: RefCell<Paths>,
}

impl<'a> Flow<'a> {
    pub(super) fn new(module: &'a Module, envs: &'a EnvLayouts, holds: &'a AnyHolds<'a>) -> Self {
        let globals = (module.data_globals.iter())
            .map(|g| (g.name.as_str(), g.ty))
            .collect();
        let fits = (module.funcs.iter().enumerate())
            .filter(|(_, f)| FITS.contains(&f.name.as_str()))
            .map(|(i, _)| FuncId(i as u32))
            .collect();
        Flow {
            module,
            envs,
            holds,
            globals,
            alloc: module.funcs.iter().position(|f| f.name == OBJ_ALLOC),
            box_alloc: module.funcs.iter().position(|f| f.name == BOX_ALLOC),
            fits,
            open: vec![false; module.struct_layouts.len()],
            paths: RefCell::new(Paths::default()),
        }
    }

    /// Add what the functions `take` picks contribute: the parameters
    /// of those entered blind, and the scan of those not dead.
    pub(super) fn scan(&mut self, take: impl Fn(usize) -> bool) {
        let module = self.module;
        for fid in (self.holds.address_taken().into_iter()).filter(|f| take(f.0 as usize)) {
            let f = &module.funcs[fid.0 as usize];
            for p in &f.params {
                self.paths
                    .borrow_mut()
                    .entered_blind(module, f.value_type(*p));
            }
        }
        for (i, f) in module.funcs.iter().enumerate() {
            if self.holds.is_dead(FuncId(i as u32)) || !take(i) {
                continue;
            }
            let mut scan = Scan {
                module,
                f,
                fid: FuncId(i as u32),
                holds: self.holds,
                globals: &self.globals,
                envs: self.envs,
                alloc: self.alloc,
                box_alloc: self.box_alloc,
                fits: &self.fits,
                allocas: HashMap::default(),
                global_refs: HashMap::default(),
                data_of: HashMap::default(),
                fn_addrs: HashMap::default(),
                env_of: HashMap::default(),
                loaded: HashMap::default(),
                tested: HashSet::default(),
                bits_of: HashMap::default(),
                open: &mut self.open,
                paths: &self.paths,
            };
            scan.run();
        }
    }

    pub(super) fn settle(self) -> (Vec<bool>, Vec<(StructId, StructId)>) {
        let paths = self.paths.into_inner();
        let widens = paths.widens();
        let mut open = self.open;
        paths.settle(&mut open);
        (open, widens)
    }
}
