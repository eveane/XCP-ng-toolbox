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
