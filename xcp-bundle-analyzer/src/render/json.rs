// json.rs - JSON report output
// Author: eveane & Vibe Code
// Date: 2026-11-02
// Open source: LGPL-2.1: free software under the GNU Lesser General Public License v2.1, as-is without warranty; see xcp-bundle-analyzer/Cargo.toml

//! JSON output (serde).

use serde::Serialize;

pub fn render_json<T: Serialize>(value: &T, pretty: bool) -> anyhow::Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}
