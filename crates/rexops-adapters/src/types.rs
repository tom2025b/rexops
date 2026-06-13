// types.rs — re-exports AdapterHealth and AdapterOutput from rexops-core.
// These types now live in core so that OpsSnapshot (a core type) can hold them
// without creating a core→adapters dependency. Keep this re-export so any code
// that imports via `crate::types::*` continues to compile unchanged.
pub use rexops_core::{AdapterHealth, AdapterOutput};
