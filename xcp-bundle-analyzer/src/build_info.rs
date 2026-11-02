// build_info.rs - compile-time version and build constants
// Author: eveane & Vibe Code
// Date: 2026-11-02
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

/// Full version string baked at compile time by build.rs.
pub const VERSION: &str = concat!(
    "v",
    env!("CARGO_PKG_VERSION"),
    " (build ",
    env!("XCP_BA_GIT_HASH", "unknown"),
    ", built ",
    env!("XCP_BA_BUILD_DATE", "unknown"),
    ")"
);
