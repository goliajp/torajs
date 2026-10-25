//! The scans of every function `cell_slot_flow` reads, added up into
//! the open sids and the prefix paths, and the two answers the pass
//! asks for at once: over every function, and over the functions that
//! run.

use std::cell::RefCell;

use crate::ssa::id_hash::{HashMap, HashSet};
use crate::ssa::{FuncId, Module, StructId, Type};

use super::super::any_holds::AnyHolds;
use super::super::pin_facts::Watch;
use super::{BOX_ALLOC, EnvLayouts, FITS, OBJ_ALLOC, Paths, Scan};

/// Per sid, whether a slot of that sid can hold a cell of another
/// layout. A slot of a prefix of an open sid is open too once a cell
/// read out of an open slot can reach it (`paths`). A boxed adapter
/// nothing references never runs, and what its body hands on reaches
/// no slot. With it, every prefix path a cell took ([`open_sids_with`]).
pub(crate) fn open_sids(
    module: &Module,
    envs: &EnvLayouts,
) -> (Vec<bool>, Vec<(StructId, StructId)>) {
    open_sids_with(module, envs, &vec![false; module.funcs.len()])
}

/// [`open_sids`] over the functions `skip` leaves, with every prefix
/// path a cell took (`(wide, narrow)`: a cell of `wide` reached a slot
/// of `narrow`).
pub(crate) fn open_sids_with(
    module: &Module,
    envs: &EnvLayouts,
    skip: &[bool],
) -> (Vec<bool>, Vec<(StructId, StructId)>) {
    let holds = AnyHolds::of(module);
    let mut flow = Flow::new(module, envs, &holds);
    flow.scan(|i| !skip[i]);
    flow.settle()
}

/// [`open_sids`] (`all`), and [`open_sids_with`] over the functions
/// neither `asleep` nor the dead adapters rule out (`skip`): the scans
/// the two share run once.
pub(crate) struct Both {
    pub all: Vec<bool>,
    /// The prefix paths of the scan behind `all`.
    pub all_widens: Vec<(StructId, StructId)>,
    pub open: Vec<bool>,
    pub widens: Vec<(StructId, StructId)>,
    pub skip: Vec<bool>,
    /// `watch` with what the functions that run showed it.
    pub pins: Option<Watch>,
    /// What the scan of the functions that run added up to, when there
    /// was a `watch` (`open_sids_more`).
    pub scanned: Option<Scanned>,
}

/// The openness and the paths a scan added up to before they were
/// settled: a scan of more functions adds to it. `dead` names the
/// functions no road reaches (`AnyHolds::is_dead`).
#[derive(Clone)]
pub(crate) struct Scanned {
    open: Vec<bool>,
    paths: Paths,
    dead: Vec<bool>,
}

impl Scanned {
    fn settled(self) -> (Vec<bool>, Vec<(StructId, StructId)>) {
        let (mut open, widens) = (self.open, self.paths.widens());
        self.paths.settle(&mut open);
        (open, widens)
    }
}

pub(crate) fn open_sids_both(
    module: &Module,
    envs: &EnvLayouts,
    asleep: &[bool],
    watch: Option<Watch>,
) -> Both {
    scan_both(module, envs, asleep, watch)
}

/// [`open_sids_both`] but `all` (left empty), over the functions that
/// run once `asleep` names fewer: `scanned` is what the functions that
/// ran before, those `before` does not rule out, added up to, and
/// `watch` what they showed it; only the others are scanned, and
/// nothing is read again when there are none.
pub(crate) fn open_sids_more(
    module: &Module,
    envs: &EnvLayouts,
    (scanned, before): (Scanned, &[bool]),
    asleep: &[bool],
    watch: Watch,
) -> Both {
    let skip: Vec<bool> = (asleep.iter().zip(&scanned.dead))
        .map(|(a, d)| *a || *d)
        .collect();
    let more = |i: usize| before[i] && !skip[i];
    let (pins, scanned) = if (0..skip.len()).any(more) {
        let holds = AnyHolds::of(module);
        let mut flow = Flow::new(module, envs, &holds);
        let dead = scanned.dead;
        (flow.open, flow.pins) = (scanned.open, Some(watch));
        flow.paths = RefCell::new(scanned.paths);
        flow.scan(more);
        let pins = flow.pins.take();
        let open = flow.open;
        let paths = flow.paths.into_inner();
        (pins, Scanned { open, paths, dead })
    } else {
        (Some(watch), scanned)
    };
    let (open, widens) = scanned.clone().settled();
    Both {
        all: Vec::new(),
        all_widens: Vec::new(),
        open,
        widens,
        skip,
        pins,
        scanned: Some(scanned),
    }
}

fn scan_both(module: &Module, envs: &EnvLayouts, asleep: &[bool], watch: Option<Watch>) -> Both {
    let holds = AnyHolds::of(module);
    let dead: Vec<bool> = (0..asleep.len())
        .map(|i| holds.is_dead(FuncId(i as u32)))
        .collect();
    let skip: Vec<bool> = (asleep.iter().zip(&dead)).map(|(a, d)| *a || *d).collect();
    let mut flow = Flow::new(module, envs, &holds);
    flow.pins = watch;
    flow.scan(|i| !skip[i]);
    let pins = flow.pins.take();
    let (mut open, paths) = (flow.open.clone(), flow.paths.borrow().clone());
    let scanned = (pins.is_some()).then(|| Scanned {
        open: open.clone(),
        paths: paths.clone(),
        dead,
    });
    flow.scan(|i| skip[i]);
    let (all, all_widens) = flow.settle();
    let widens = paths.widens();
    paths.settle(&mut open);
    Both {
        all,
        all_widens,
        open,
        widens,
        skip,
        pins,
        scanned,
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
    pins: Option<Watch>,
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
            pins: None,
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
                pins: self.pins.as_mut(),
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
