use std::collections::BTreeMap;

/// Key/value parser for `/etc/xensource-inventory` shell-style files
/// (collected in every status report as `etc/xensource-inventory`).
#[derive(Default, Debug, Clone)]
pub struct Inventory(pub BTreeMap<String, String>);

impl Inventory {
    pub fn parse(text: &str) -> Inventory {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, val) = match line.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            let val = val.trim();
            let val = val
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| val.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(val);
            map.insert(key.trim().to_string(), val.to_string());
        }
        Inventory(map)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|s| s.as_str())
    }

    #[allow(dead_code)]
    pub fn str_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }

    /// Dom0 configured memory in MiB.
    pub fn dom0_memory_mib(&self) -> Option<u64> {
        self.get("CONTROL_DOMAIN_MEMORY")
            .and_then(|v| v.parse().ok())
    }

    pub fn dom0_cpus(&self) -> Option<u32> {
        self.get("CONTROL_DOMAIN_CPUS").and_then(|v| v.parse().ok())
    }
}
