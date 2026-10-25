use super::ContainerError;
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, atomic::AtomicU64},
    thread::ThreadId,
};

pub(super) type Erased = dyn Any + Send + Sync;
pub(super) type Factory =
    Arc<dyn Fn(&super::Container) -> Result<Arc<Erased>, ContainerError> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lifetime {
    Singleton,
    Transient,
}

pub(super) struct Registration {
    pub(super) name: &'static str,
    pub(super) lifetime: Lifetime,
    pub(super) factory: Factory,
    pub(super) value: Option<Arc<Erased>>,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) registrations: HashMap<TypeId, Registration>,
    pub(super) construction_order: Vec<TypeId>,
    pub(super) resolving: HashMap<TypeId, ThreadId>,
    pub(super) waiting_for: HashMap<ThreadId, ThreadId>,
    pub(super) sealed: bool,
    pub(super) stopped: bool,
}

pub(super) static NEXT_CONTAINER_ID: AtomicU64 = AtomicU64::new(1);
thread_local! {
    pub(super) static RESOLUTION_STACK: RefCell<Vec<(u64, TypeId, &'static str)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn ensure_registrable(state: &State) -> Result<(), ContainerError> {
    if state.stopped {
        Err(ContainerError::Stopped)
    } else if state.sealed {
        Err(ContainerError::Sealed)
    } else {
        Ok(())
    }
}
