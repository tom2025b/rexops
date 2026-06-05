//! models — Domain data types shared by the feed adapters.
//!
//! These modules hold the *data-only* types previously embedded in the now-deleted
//! raw feed adapter files (toolfoundry.rs, scriptvault.rs, bulwark_feed.rs).
//! Moving them here separates "type definitions" from "feed reading logic", which
//! is moot now that the raw feed adapters are gone, but keeps the types findable
//! under a clear name and under the 300-line god-file limit.
//!
//! The three modules mirror the three Workstate snapshot sections they populate:
//!   - tools     → ToolFoundryInfo, Tool
//!   - scripts   → ScriptVaultInfo, Script
//!   - findings  → BulwarkScanInfo, ScanItem, Severity, RiskTally

pub mod findings;
pub mod scripts;
pub mod tools;
