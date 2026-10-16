use crate::report::{BundleReport, ConsolidatedReport};
use serde::Serialize;
use std::fmt::Write as _;

fn gib(bytes: u64) -> String {
    if bytes == 0 {
        return "0".to_string();
    }
    format!("{:.2} GiB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn mib(bytes: u64) -> String {
    format!("{:.0} MiB", bytes as f64 / 1024.0 / 1024.0)
}

pub fn render_text(r: &BundleReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== Status report analysis: {} ===", r.bundle_path);
    let _ = writeln!(out, "Pool: {} (uuid {}), master: {}", r.pool.name, r.pool.uuid, r.pool.master);
    let _ = writeln!(
        out,
        "Hosts: {}   VMs: {} running / {} total   Networks: {}",
        r.pool.host_count, r.pool.vm_running, r.pool.vm_total, r.networks
    );
    let _ = writeln!(
        out,
        "SR storage: {} total, {} used   VDIs total size: {}",
        gib(r.pool.sr_total_size_bytes),
        gib(r.pool.sr_total_used_bytes),
        gib(r.pool.vdi_total_size_bytes)
    );

    let _ = writeln!(out, "\n--- HOSTS ({}) ---", r.hosts.len());
    for h in &r.hosts {
        let _ = writeln!(
            out,
            "{} [{}] {}",
            h.hostname,
            h.uuid,
            if h.enabled { "enabled" } else { "disabled" }
        );
        let _ = writeln!(
            out,
            "  address: {}   state: {}   ref: {}",
            h.address, h.power_state, h.opaque_ref
        );
        let _ = writeln!(
            out,
            "  product: {}   xen: {}   kernel: {}",
            h.product_version, h.xen_version, h.kernel_version
        );
        let _ = writeln!(
            out,
            "  cpu: {} x {} ({} sockets, {} cores/socket)",
            h.cpu_count, h.cpu_model, h.sockets, h.cores_per_socket
        );
        let _ = writeln!(
            out,
            "  memory: {} total / {} free   dom0 config: {}, {} CPUs",
            gib(h.memory_total_bytes),
            gib(h.memory_free_bytes),
            h.dom0_memory_config_mib.map(|m| format!("{m} MiB")).unwrap_or_else(|| "n/a".into()),
            h.dom0_cpus_config.map(|c| c.to_string()).unwrap_or_else(|| "n/a".into())
        );
        let _ = writeln!(out, "  PBDs: {}   PIFs: {}", h.pbd_count, h.pif_count);
    }

    let _ = writeln!(out, "\n--- STORAGE REPOSITORIES ({}) ---", r.srs.len());
    for s in &r.srs {
        let _ = writeln!(
            out,
            "{} [{}] type={} shared={} size={} used={} PBDs={} VDIs={}",
            s.name,
            s.uuid,
            s.sr_type,
            s.shared,
            gib(s.size_bytes),
            gib(s.used_bytes),
            s.pbd_count,
            s.vdi_count
        );
    }

    let _ = writeln!(out, "\n--- VIRTUAL MACHINES ({}) ---", r.vms.len());
    for v in &r.vms {
        let _ = writeln!(
            out,
            "{} [{}] {}{} vCPUs={} mem={} host={} VIFs={} VBDs={}",
            v.name,
            v.uuid,
            v.power_state,
            if v.is_template { "(template)" } else { "" },
            v.vcpus_max,
            mib(v.memory_dynamic_max_bytes),
            v.resident_host,
            v.vif_count,
            v.vbd_count
        );
        if !v.os_name.is_empty() {
            let _ = writeln!(out, "    os: {}", v.os_name);
        }
    }

    let _ = writeln!(out, "\n--- VDIs ({}) ---", r.vdis.len());
    for d in &r.vdis {
        let _ = writeln!(
            out,
            "{} [{}] type={} size={} sr={} vm={}",
            d.name, d.uuid, d.vdi_type, gib(d.size_bytes), d.sr, d.vm
        );
    }

    let _ = writeln!(out, "\n--- VBDs ({}) ---", r.vbds.len());
    for b in &r.vbds {
        let _ = writeln!(
            out,
            "{} vm={} vdi={} dev={} type={} mode={} attached={} bootable={}",
            b.uuid, b.vm, b.vdi, b.device, b.vbd_type, b.mode, b.attached, b.bootable
        );
    }

    let _ = writeln!(out, "\n--- PIFs ({}) ---", r.pifs.len());
    for p in &r.pifs {
        let _ = writeln!(
            out,
            "{} {} host={} net={} dev={} ip={} mac={} vlan={} attached={}",
            p.uuid, p.opaque_ref, p.host, p.network, p.device, p.address, p.mac,
            p.vlan.map(|v| v.to_string()).unwrap_or_else(|| "-".into()),
            p.attached
        );
        if p.management {
            let _ = writeln!(out, "    (management)");
        }
    }

    let _ = writeln!(out, "\n--- VIFs ({}) ---", r.vifs.len());
    for v in &r.vifs {
        let _ = writeln!(
            out,
            "{} vm={} net={} dev={} mac={} attached={}",
            v.uuid, v.vm, v.network, v.device, v.mac, v.attached
        );
    }

    for w in &r.warnings {
        let _ = writeln!(out, "WARNING: {w}");
    }
    out
}

pub fn render_consolidated_text(c: &ConsolidatedReport) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "=== Consolidated pool view ({} bundles) ===", c.bundles.len());
    let _ = writeln!(out, "Pool: {} (uuid {}), master: {}", c.pool.name, c.pool.uuid, c.pool.master);
    let _ = writeln!(
        out,
        "Hosts: {}   VMs: {} running / {} total",
        c.pool.host_count, c.pool.vm_running, c.pool.vm_total
    );
    let _ = writeln!(
        out,
        "SR storage: {} total, {} used",
        gib(c.pool.sr_total_size_bytes),
        gib(c.pool.sr_total_used_bytes)
    );
    for b in &c.bundles {
        let _ = writeln!(out, "\n### bundle: {}", b.bundle_path);
        let _ = writeln!(
            out,
            "    hosts: {}, VMs running: {}/{}",
            b.pool.host_count, b.pool.vm_running, b.pool.vm_total
        );
    }
    let _ = writeln!(out, "\n--- POOL HOSTS ({} unique) ---", c.hosts.len());
    for h in &c.hosts {
        let _ = writeln!(
            out,
            "{} [{}] product={} xen={} mem={} dom0={} PBDs={}",
            h.hostname,
            h.uuid,
            h.product_version,
            h.xen_version,
            gib(h.memory_total_bytes),
            h.dom0_memory_config_mib.map(|m| format!("{m} MiB")).unwrap_or_else(|| "n/a".into()),
            h.pbd_count
        );
    }
    out
}

pub fn render_json<T: Serialize>(value: &T, pretty: bool) -> anyhow::Result<String> {
    if pretty {
        Ok(serde_json::to_string_pretty(value)?)
    } else {
        Ok(serde_json::to_string(value)?)
    }
}

/// Single-file HTML dashboard, XO-like: embeds the JSON report and renders
/// client-side with plain JS (no external assets, works offline).
pub fn render_html(r: &BundleReport) -> anyhow::Result<String> {
    let json = serde_json::to_string_pretty(r)?;
    Ok(HTML_TEMPLATE.replace("__REPORT_JSON__", &json))
}

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>xcp-bundle-analyzer</title>
<style>
:root { --accent:#1f7cd8; --bg:#f4f6f8; --card:#fff; --text:#22303c; --muted:#7d8b99; }
* { box-sizing: border-box; }
body { margin:0; font-family: -apple-system, "Segoe UI", Roboto, sans-serif; background:var(--bg); color:var(--text); }
header { background:var(--accent); color:#fff; padding:18px 28px; }
header h1 { margin:0; font-size:1.2rem; }
header .sub { opacity:.85; font-size:.85rem; margin-top:4px; }
main { padding:24px; max-width:1200px; margin:0 auto; }
.cards { display:grid; grid-template-columns:repeat(auto-fit,minmax(180px,1fr)); gap:14px; }
.card { background:var(--card); border-radius:8px; padding:16px; box-shadow:0 1px 3px rgba(0,0,0,.08); }
.card .label { color:var(--muted); font-size:.75rem; text-transform:uppercase; letter-spacing:.05em; }
.card .value { font-size:1.5rem; font-weight:600; margin-top:4px; }
section { margin-top:26px; }
h2 { font-size:1rem; margin:0 0 10px; }
table { width:100%; border-collapse:collapse; background:var(--card); border-radius:8px; overflow:hidden; box-shadow:0 1px 3px rgba(0,0,0,.08); }
th, td { text-align:left; padding:8px 12px; font-size:.82rem; border-bottom:1px solid #eef2f5; }
th { background:#eef3f7; font-weight:600; }
tr:last-child td { border-bottom:none; }
.badge { display:inline-block; padding:1px 8px; border-radius:10px; font-size:.72rem; background:#e3eefb; color:#1f5fa9; }
.badge.run { background:#e0f2e4; color:#1c7a2f; }
.badge.off { background:#f3e3e3; color:#a33; }
.badge.tpl { background:#efe8f7; color:#6a4bb0; }
.muted { color:var(--muted); }
input#filter { width:260px; padding:6px 10px; border:1px solid #d5dde4; border-radius:6px; margin-left:12px; }
</style>
</head>
<body>
<header>
  <h1>XCP-ng / XenServer status report</h1>
  <div class="sub" id="subtitle"></div>
</header>
<main>
  <div class="cards" id="cards"></div>
  <section><h2>Hosts</h2><table id="hosts"><thead></thead><tbody></tbody></table></section>
  <section><h2>Storage repositories <input id="filter" placeholder="filter VMs by name..." oninput="filterVms()"></h2>
    <table id="srs"><thead></thead><tbody></tbody></table></section>
  <section><h2>Virtual machines</h2><table id="vms"><thead></thead><tbody></tbody></table></section>
  <section><h2>VDIs</h2><table id="vdis"><thead></thead><tbody></tbody></table></section>
  <section><h2>VBDs</h2><table id="vbds"><thead></thead><tbody></tbody></table></section>
  <section><h2>PIFs</h2><table id="pifs"><thead></thead><tbody></tbody></table></section>
  <section><h2>VIFs</h2><table id="vifs"><thead></thead><tbody></tbody></table></section>
</main>
<script type="application/json" id="report">__REPORT_JSON__</script>
<script>
const r = JSON.parse(document.getElementById('report').textContent);
const fmtGib = b => b ? (b/1024**3).toFixed(1)+' GiB' : '0';
const fmtMib = b => b ? (b/1024**2).toFixed(0)+' MiB' : '-';
document.getElementById('subtitle').textContent = r.bundle_path + ' — pool ' + r.pool.name + ' (' + r.pool.uuid + ')';
document.getElementById('cards').innerHTML = [
  ['Hosts', r.pool.host_count], ['VMs running', r.pool.vm_running + ' / ' + r.pool.vm_total],
  ['SRs', r.srs.length], ['Storage', fmtGib(r.pool.sr_total_size_bytes)],
  ['Used', fmtGib(r.pool.sr_total_used_bytes)], ['Networks', r.networks],
].map(([l,v]) => `<div class="card"><div class="label">${l}</div><div class="value">${v}</div></div>`).join('');

function fill(id, cols, rows) {
  const t = document.getElementById(id);
  t.querySelector('thead').innerHTML = '<tr>' + cols.map(c=>`<th>${c}</th>`).join('') + '</tr>';
  t.querySelector('tbody').innerHTML = rows.map(cells => '<tr>' + cells.join('') + '</tr>').join('');
}
const esc = s => String(s??'').replace(/[&<>"]/g, c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const badge = (state, tpl) => tpl ? '<span class="badge tpl">template</span>'
  : `<span class="badge ${state==='Running'?'run':state==='Halted'?'off':''}">${esc(state)}</span>`;

fill('hosts', ['Name','UUID','Address','Product','Xen','CPU','Cores','Memory','Free','dom0 config','PBDs','PIFs'],
  r.hosts.map(h=>[`<td>${esc(h.hostname)}</td>`,`<td class="muted">${esc(h.uuid)}</td>`,`<td>${esc(h.address)}</td>`,
    `<td>${esc(h.product_version)}</td>`,`<td>${esc(h.xen_version)}</td>`,`<td>${esc(h.cpu_model)}</td>`,
    `<td>${h.cpu_count}</td>`,`<td>${fmtGib(h.memory_total_bytes)}</td>`,`<td>${fmtGib(h.memory_free_bytes)}</td>`,
    `<td>${h.dom0_memory_config_mib ?? '-'} MiB / ${h.dom0_cpus_config ?? '-'} vCPU</td>`,`<td>${h.pbd_count}</td>`,`<td>${h.pif_count}</td>`]));
fill('srs', ['Name','UUID','Type','Shared','Size','Used','PBDs','VDIs'],
  r.srs.map(s=>[`<td>${esc(s.name)}</td>`,`<td class="muted">${esc(s.uuid)}</td>`,`<td>${esc(s.sr_type)}</td>`,
    `<td>${s.shared}</td>`,`<td>${fmtGib(s.size_bytes)}</td>`,`<td>${fmtGib(s.used_bytes)}</td>`,`<td>${s.pbd_count}</td>`,`<td>${s.vdi_count}</td>`]));
fill('vms', ['Name','UUID','State','vCPUs','Memory','Host','VIFs','VDIs','OS'],
  r.vms.map(v=>[`<td>${esc(v.name)}</td>`,`<td class="muted">${esc(v.uuid)}</td>`, `<td>${badge(v.power_state, v.is_template)}</td>`,
    `<td>${v.vcpus_max}</td>`,`<td>${fmtMib(v.memory_dynamic_max_bytes)}</td>`,`<td>${esc(v.resident_host)}</td>`,
    `<td>${v.vif_count}</td>`,`<td>${v.vdi_count}</td>`,`<td>${esc(v.os_name)}</td>`]));
fill('vdis', ['Name','UUID','Type','Size','SR','VM'],
  r.vdis.map(d=>[`<td>${esc(d.name)}</td>`,`<td class="muted">${esc(d.uuid)}</td>`,`<td>${esc(d.vdi_type)}</td>`,
    `<td>${fmtGib(d.size_bytes)}</td>`,`<td>${esc(d.sr)}</td>`,`<td>${esc(d.vm)}</td>`]));
fill('vbds', ['VM','VDI','Device','Type','Mode','Attached','Bootable'],
  r.vbds.map(b=>[`<td>${esc(b.vm)}</td>`,`<td>${esc(b.vdi)}</td>`,`<td>${esc(b.device)}</td>`,
    `<td>${esc(b.vbd_type)}</td>`,`<td>${esc(b.mode)}</td>`,`<td>${b.attached}</td>`,`<td>${b.bootable}</td>`]));
fill('pifs', ['Host','Network','Device','IP','MAC','VLAN','Management','Attached'],
  r.pifs.map(p=>[`<td>${esc(p.host)}</td>`,`<td>${esc(p.network)}</td>`,`<td>${esc(p.device)}</td>`,`<td>${esc(p.address)}</td>`,
    `<td class="muted">${esc(p.mac)}</td>`,`<td>${p.vlan ?? '-'}</td>`,`<td>${p.management}</td>`,`<td>${p.attached}</td>`]));
fill('vifs', ['VM','Network','Device','MAC','Attached'],
  r.vifs.map(v=>[`<td>${esc(v.vm)}</td>`,`<td>${esc(v.network)}</td>`,`<td>${esc(v.device)}</td>`,
    `<td class="muted">${esc(v.mac)}</td>`,`<td>${v.attached}</td>`]));

function filterVms() {
  const q = document.getElementById('filter').value.toLowerCase();
  document.querySelectorAll('#vms tbody tr').forEach(tr => {
    tr.style.display = tr.textContent.toLowerCase().includes(q) ? '' : 'none';
  });
}
</script>
</body>
</html>
"#;
