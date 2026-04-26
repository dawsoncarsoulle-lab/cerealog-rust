pub mod analytics;
pub mod artifacts;
pub mod errors;
pub mod logs;
pub mod packages;

#[derive(Clone, Copy, PartialEq)]
pub enum Tab {
    Logs,
    Artifacts,
    Packages,
    DeployErrors,
    ExecErrors,
    Analytics,
}

impl Tab {
    pub fn all() -> &'static [Tab] {
        &[
            Tab::Logs,
            Tab::Artifacts,
            Tab::Packages,
            Tab::DeployErrors,
            Tab::ExecErrors,
            Tab::Analytics,
        ]
    }

    pub fn index(self) -> usize {
        Tab::all().iter().position(|&t| t == self).unwrap_or(0)
    }
}
