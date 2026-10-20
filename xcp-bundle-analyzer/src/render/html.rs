//! Self-contained HTML dashboard.

use crate::build_info::VERSION;
use crate::report::BundleReport;

/// Single-file HTML dashboard, XO-like: embeds the JSON report and renders
/// client-side with plain JS (no external assets, works offline).
pub fn render_html(r: &BundleReport) -> anyhow::Result<String> {
    let json = serde_json::to_string_pretty(r)?;
    Ok(HTML_TEMPLATE
        .replace("__REPORT_JSON__", &json)
        .replace("__VERSION__", VERSION))
}

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>xcp-bundle-analyzer __VERSION__</title>
<style>
:root { --accent:#1f7cd8; --bg:#f4f6f8; --card:#fff; --text:#22303c; --muted:#7d8b99; }
* { box-sizing: border-box; }
body { margin:0; font-family: -apple-system, "Segoe UI", Roboto, sans-serif; background:var(--bg); color:var(--text); }
header { background:var(--accent); color:#fff; padding:18px 28px; }
header h1 { margin:0; font-size:1.2rem; }
header .sub { opacity:.85; font-size:.85rem; margin-top:4px; }
header .ver { opacity:.7; font-size:.72rem; margin-top:2px; }
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
.badge.warn { background:#fdf3d7; color:#8a6d1a; }
.muted { color:var(--muted); }
input#filter { width:260px; padding:6px 10px; border:1px solid #d5dde4; border-radius:6px; margin-left:12px; }
</style>
</head>
<body>
<header>
  <h1>XCP-ng / XenServer status report</h1>
  <div class="sub" id="subtitle"></div>
  <div class="ver" id="verline"></div>
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
document.getElementById('verline').textContent = 'xcp-bundle-analyzer __VERSION__';
document.getElementById('subtitle').textContent = r.bundle_path + ' — pool ' + r.pool.name + ' (' + r.pool.uuid + ')'
  + (r.bundle_origin_host ? ' — collected from ' + r.bundle_origin_host : '');
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
const rawFill = fill;
const esc = s => String(s??'').replace(/[&<>"]/g, c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const badge = (state, tpl) => tpl ? '<span class="badge tpl">template</span>'
  : `<span class="badge ${state==='Running'?'run':state==='Halted'?'off':''}">${esc(state)}</span>`;

fill('hosts', ['Name','UUID','Address','Product','Xen','CPU','Cores/socket','Memory','Free','dom0','Uptime','VMs','vCPUs','PBDs','PIFs','Note'],
  r.hosts.map(h=>[`<td>${esc(h.hostname)}${h.is_bundle_origin?' <span class="badge">bundle origin</span>':''}</td>`,`<td class="muted">${esc(h.uuid)}</td>`,`<td>${esc(h.address)}</td>`,
    `<td>${esc(h.product_version)}</td>`,`<td>${esc(h.xen_version)}</td>`,`<td>${esc(h.cpu_model)}</td>`,
    `<td>${h.cores_per_socket}</td>`,`<td>${fmtGib(h.memory_total_bytes)}</td>`,
    `<td>${fmtGib(h.memory_free_bytes)}${h.mem_free_stale?' <span class="badge warn">n/a</span>':''}</td>`,
    `<td>${(h.dom0_memory_config_mib ?? (h.dom0_memory_live_bytes ? Math.round(h.dom0_memory_live_bytes/1024**2) : '-'))} MiB</td>`,
    `<td>${h.uptime_seconds!=null ? Math.floor(h.uptime_seconds/86400)+'d '+Math.floor(h.uptime_seconds%86400/3600)+'h' : '-'}${h.boot_time!=null ? '<br><span class="muted">boot '+new Date(h.boot_time*1000).toISOString().replace('T',' ').slice(0,16)+' UTC</span>' : ''}</td>`,
    `<td>${h.vms_running} run / ${h.vms_resident}</td>`,`<td>${h.vcpus_in_use}</td>`,`<td>${h.pbd_count}</td>`,`<td>${h.pif_count}</td>`,
    `<td>${h.is_bundle_origin?'this bundle was collected here':''}</td>`]));
fill('srs', ['Name','UUID','Type','Shared','Size','Used','PBDs','VDIs','auto-scan-interval','Attached hosts'],
  r.srs.map(s=>[`<td>${esc(s.name)}</td>`,`<td class="muted">${esc(s.uuid)}</td>`,`<td>${esc(s.sr_type)}</td>`,
    `<td>${s.shared}</td>`,`<td>${fmtGib(s.size_bytes)}</td>`,`<td>${fmtGib(s.used_bytes)}</td>`,`<td>${s.pbd_count}</td>`,`<td>${s.vdi_count}</td>`,
    `<td>${s.auto_scan_interval!=null?`<span class="badge warn">${s.auto_scan_interval}s</span>`:'-'}</td>`,
    `<td class="muted">${esc((s.hosts||[]).join(', '))}</td>`]));
const isRealVm = v => !v.is_template && !v.is_snapshot && !v.is_vmss_snapshot && !v.is_control_domain;
const vmRow = v => `<tr><td>${esc(v.name)}</td><td class="muted">${esc(v.uuid)}</td><td>${badge(v.power_state, false)}</td>
    <td>${v.vcpus_max}</td><td>${fmtMib(v.memory_dynamic_max_bytes)}</td><td>${esc(v.resident_host)}</td>
    <td>${v.vif_count}</td><td>${v.vbd_count}</td><td>${esc(v.os_name)}</td></tr>`;
const groups = {};
r.vms.filter(isRealVm).forEach(v => { (groups[v.resident_host||'(not running)'] ||= []).push(v); });
let vmRows = [];
Object.keys(groups).sort().forEach(host => {
  const g = groups[host].sort((a,b)=>a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  const running = g.filter(v=>v.power_state==='Running');
  const vcpus = running.reduce((s,v)=>s+Math.max(v.vcpus_at_startup,v.vcpus_max),0);
  vmRows.push(`<tr><td colspan="9" style="background:#e8f0fa;font-weight:600">${esc(host)} — ${running.length} running, ${vcpus} vCPUs</td></tr>`);
  g.forEach(v => vmRows.push(vmRow(v)));
});
r.vms.filter(v=>!isRealVm(v))
  .sort((a,b)=>a.name.toLowerCase().localeCompare(b.name.toLowerCase()))
  .forEach(v => vmRows.push(`<tr><td>${esc(v.name)}</td><td class="muted">${esc(v.uuid)}</td>
    <td><span class="badge tpl">${v.is_template?'template'+(v.is_default_template?' (default)':''):'snapshot'}</span></td>
    <td>${v.vcpus_max}</td><td>${fmtMib(v.memory_static_max_bytes)}</td><td></td><td></td><td>${v.vbd_count}</td><td></td></tr>`));
fill('vms', ['Name','UUID','State','vCPUs','Memory','Host','VIFs','VBDs','OS'], []);
document.getElementById('vms').querySelector('tbody').innerHTML = vmRows.join('');
fill('vdis', ['Name','UUID','Type','Size','SR','VM'],
  r.vdis.map(d=>[`<td>${esc(d.name)}</td>`,`<td class="muted">${esc(d.uuid)}</td>`,`<td>${esc(d.vdi_type)}</td>`,
    `<td>${fmtGib(d.size_bytes)}</td>`,`<td>${esc(d.sr)}</td>`,`<td>${esc(d.vm)}</td>`]));
fill('vbds', ['VM','VDI','Device','Type','Mode','Attached','Bootable'],
  r.vbds.map(b=>[`<td>${esc(b.vm)}</td>`,`<td>${esc(b.vdi)}</td>`,`<td>${esc(b.device)}</td>`,
    `<td>${esc(b.vbd_type)}</td>`,`<td>${esc(b.mode)}</td>`,`<td>${b.attached}</td>`,`<td>${b.bootable}</td>`]));
fill('pifs', ['Host','Network','Device','IP','MAC','VLAN','Mgmt','Attached'],
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

// ---------------------------------------------------------------------------
// vm-display: detailed single-VM view (like `xe vm-param-list`)
// ---------------------------------------------------------------------------
