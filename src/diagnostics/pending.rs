#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureStatus {
    NotImplemented,
}

pub fn configure_remote_logging(_endpoint: &str) -> FeatureStatus {
    FeatureStatus::NotImplemented
}
pub fn enable_fatal_system_capture() -> FeatureStatus {
    FeatureStatus::NotImplemented
}
