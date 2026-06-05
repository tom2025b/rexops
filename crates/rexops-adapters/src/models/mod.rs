//! models — Domain data types for Workstate snapshot sections.
//!
//! These modules hold the data-only types used by the Workstate consumer.
//!
//! The three modules mirror the three Workstate snapshot sections they populate:
//!   - tools     → ToolsInfo, Tool
//!   - scripts   → ScriptsInfo, Script
//!   - findings  → FindingsInfo, ScanItem, Severity, RiskTally

pub mod findings;
pub mod scripts;
pub mod tools;
