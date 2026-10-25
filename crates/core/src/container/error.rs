#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    Sealed,
    Stopped,
    Duplicate(&'static str),
    Missing(&'static str),
    WrongLifetime {
        service: &'static str,
        requested: &'static str,
    },
    Circular(String),
    FactoryFailed {
        service: &'static str,
        reason: String,
    },
}

impl std::fmt::Display for ContainerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sealed => f.write_str("Cannot register a service after container seal"),
            Self::Stopped => f.write_str("Cannot use a stopped container"),
            Self::Duplicate(name) => write!(f, "Duplicate service registration: {name}"),
            Self::Missing(name) => write!(f, "Service is not registered: {name}"),
            Self::WrongLifetime { service, requested } => {
                write!(f, "Service {service} must be resolved with {requested}")
            }
            Self::Circular(chain) => write!(f, "Circular service dependency: {chain}"),
            Self::FactoryFailed { service, reason } => {
                write!(f, "Factory for {service} failed: {reason}")
            }
        }
    }
}
impl std::error::Error for ContainerError {}
