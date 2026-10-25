use super::{
    Container, ContainerError,
    state::{Erased, Lifetime, RESOLUTION_STACK},
};
use std::{
    any::{Any, TypeId, type_name},
    sync::Arc,
    thread,
};

pub(super) fn resolve<T: Any + Send + Sync>(
    container: &Container,
    requested: Lifetime,
) -> Result<Arc<T>, ContainerError> {
    let id = TypeId::of::<T>();
    let name = type_name::<T>();
    let current_thread = thread::current().id();

    loop {
        let is_reentrant = RESOLUTION_STACK.with(|stack| {
            stack
                .borrow()
                .iter()
                .any(|(owner, item, _)| *owner == container.id && *item == id)
        });
        if is_reentrant {
            return Err(circular_error(container, id, name));
        }

        let factory = {
            let mut state = container.lock();
            if state.stopped {
                return Err(ContainerError::Stopped);
            }
            state.sealed = true;
            let Some(registration) = state.registrations.get(&id) else {
                return Err(ContainerError::Missing(name));
            };
            if registration.lifetime != requested {
                return Err(ContainerError::WrongLifetime {
                    service: registration.name,
                    requested: if requested == Lifetime::Singleton {
                        "get"
                    } else {
                        "make"
                    },
                });
            }
            if let Some(value) = registration.value.as_ref() {
                return Arc::clone(value)
                    .downcast::<T>()
                    .map_err(|_| ContainerError::Missing(name));
            }
            if requested == Lifetime::Singleton {
                if let Some(owner) = state.resolving.get(&id).copied() {
                    if owner == current_thread {
                        return Err(circular_error(container, id, name));
                    }
                    state.waiting_for.insert(current_thread, owner);
                    let mut cursor = owner;
                    let mut cycle = false;
                    while let Some(next) = state.waiting_for.get(&cursor).copied() {
                        if next == current_thread {
                            cycle = true;
                            break;
                        }
                        if next == cursor {
                            break;
                        }
                        cursor = next;
                    }
                    if cycle {
                        state.waiting_for.remove(&current_thread);
                        return Err(ContainerError::Circular(format!(
                            "cross-thread dependency cycle while resolving {name}"
                        )));
                    }
                    state = container
                        .changed
                        .wait(state)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    state.waiting_for.remove(&current_thread);
                    drop(state);
                    continue;
                }
                state.resolving.insert(id, current_thread);
            }
            Arc::clone(
                &state
                    .registrations
                    .get(&id)
                    .expect("registration checked above")
                    .factory,
            )
        };

        RESOLUTION_STACK.with(|stack| stack.borrow_mut().push((container.id, id, name)));
        let _guard = ResolutionGuard {
            container,
            id,
            singleton: requested == Lifetime::Singleton,
        };
        let value = factory(container)?;
        let value = value
            .downcast::<T>()
            .map_err(|_| ContainerError::FactoryFailed {
                service: name,
                reason: "factory returned a different type".into(),
            })?;

        if requested == Lifetime::Singleton {
            let mut state = container.lock();
            if state.stopped {
                return Err(ContainerError::Stopped);
            }
            if let Some(registration) = state.registrations.get_mut(&id) {
                if let Some(existing) = registration.value.as_ref() {
                    return Arc::clone(existing)
                        .downcast::<T>()
                        .map_err(|_| ContainerError::Missing(name));
                }
                let erased: Arc<Erased> = value.clone();
                registration.value = Some(erased);
                state.construction_order.push(id);
            }
        }
        return Ok(value);
    }
}

fn circular_error(container: &Container, id: TypeId, name: &'static str) -> ContainerError {
    let chain = RESOLUTION_STACK.with(|stack| {
        let stack = stack.borrow();
        let start = stack
            .iter()
            .position(|(owner, item, _)| *owner == container.id && *item == id)
            .unwrap_or(stack.len());
        let mut names: Vec<_> = stack[start..]
            .iter()
            .filter(|(owner, _, _)| *owner == container.id)
            .map(|(_, _, item)| *item)
            .collect();
        names.push(name);
        names.join(" -> ")
    });
    ContainerError::Circular(chain)
}

struct ResolutionGuard<'a> {
    container: &'a Container,
    id: TypeId,
    singleton: bool,
}

impl Drop for ResolutionGuard<'_> {
    fn drop(&mut self) {
        RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            if stack
                .last()
                .is_some_and(|(owner, item, _)| *owner == self.container.id && *item == self.id)
            {
                stack.pop();
            }
        });
        if self.singleton {
            let mut state = self.container.lock();
            state.resolving.remove(&self.id);
            state.waiting_for.remove(&thread::current().id());
            self.container.changed.notify_all();
        }
    }
}
