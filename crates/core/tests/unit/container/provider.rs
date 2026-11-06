use super::*;
use std::{
    io,
    sync::{Arc, Mutex},
};

struct Probe {
    name: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    fail_boot: bool,
}

impl ServiceProvider for Probe {
    fn register_services(&mut self, _services: &Container) -> Result<(), ProviderError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("register:{}", self.name));
        Ok(())
    }

    fn boot(&mut self, _services: &Container) -> Result<(), ProviderError> {
        self.events
            .lock()
            .unwrap()
            .push(format!("boot:{}", self.name));
        if self.fail_boot {
            Err(Box::new(io::Error::other("boot failed")))
        } else {
            Ok(())
        }
    }

    fn shutdown(&mut self, _services: &Container) {
        self.events
            .lock()
            .unwrap()
            .push(format!("shutdown:{}", self.name));
    }
}

#[test]
fn provider_lifecycle_registers_forward_and_cleans_attempted_boots_in_reverse() {
    let services = Container::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let providers: ServiceProviderList = vec![
        Box::new(Probe {
            name: "application",
            events: Arc::clone(&events),
            fail_boot: false,
        }),
        Box::new(Probe {
            name: "framework",
            events: Arc::clone(&events),
            fail_boot: true,
        }),
        Box::new(Probe {
            name: "later",
            events: Arc::clone(&events),
            fail_boot: false,
        }),
    ];
    {
        let mut lifecycle = ProviderLifecycle::new(&services, providers);
        lifecycle.register_services().unwrap();
        let error = lifecycle.boot().unwrap_err();
        assert_eq!(error.index, 1);
        assert_eq!(error.phase, "boot");
    }
    assert_eq!(
        *events.lock().unwrap(),
        [
            "register:application",
            "register:framework",
            "register:later",
            "boot:application",
            "boot:framework",
            "shutdown:framework",
            "shutdown:application",
        ]
    );
}

#[test]
fn provider_lifecycle_rejects_repeated_or_out_of_order_transitions() {
    let services = Container::new();
    let mut lifecycle = ProviderLifecycle::new(&services, Vec::new());
    assert_eq!(lifecycle.boot().unwrap_err().phase, "boot");
    lifecycle.register_services().unwrap();
    assert_eq!(lifecycle.register_services().unwrap_err().phase, "register");
    lifecycle.boot().unwrap();
    assert_eq!(lifecycle.boot().unwrap_err().phase, "boot");
    lifecycle.shutdown();
    lifecycle.shutdown();
    assert_eq!(lifecycle.boot().unwrap_err().phase, "boot");
}
