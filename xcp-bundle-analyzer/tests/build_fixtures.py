#!/usr/bin/env python3
"""Build synthetic bugtool bundle fixtures (tar.bz2 and zip) for testing."""
import io
import os
import tarfile
import zipfile
import bz2

OUT = os.path.join(os.path.dirname(__file__), "fixtures")

XAPI_DB = """<?xml version="1.0"?>
<database>
<table name="pool">
<row ref="OpaqueRef:pool1" uuid="pool-uuid-1" name__label="mypool" master="OpaqueRef:host1"/>
</table>
<table name="host">
<row ref="OpaqueRef:host1" uuid="host-uuid-1" name__label="host1" hostname="host1.example.com" address="192.168.1.11" enabled="true" power_state="Running" metrics="OpaqueRef:hm1" other_config="(('boot_time' '1750000000'))" PBDs="('OpaqueRef:pbd1' 'OpaqueRef:pbd3')" PIFs="('OpaqueRef:pif1')" cpu_info="(('cpu_count' '8') ('cores_per_socket' '4') ('socket_count' '2') ('modelname' 'Intel(R)%.Xeon(R)%.CPU%.E5-2630'))" software_version="(('product_version' '8.3.0') ('xen_version' '4.17') ('linux_version' '4.19.0-1.el8.xcpng'))"/>
<row ref="OpaqueRef:host2" uuid="host-uuid-2" name__label="host2" hostname="host2.example.com" address="192.168.1.12" enabled="true" power_state="Running" metrics="OpaqueRef:hm2" PBDs="('OpaqueRef:pbd2' 'OpaqueRef:pbd4')" PIFs="()" cpu_info="(('cpu_count' '8') ('cores_per_socket' '4') ('socket_count' '2') ('modelname' 'Intel(R)%.Xeon(R)%.CPU%.E5-2630'))" software_version="(('product_version' '8.3.0') ('xen_version' '4.17') ('linux_version' '4.19.0-1.el8.xcpng'))"/>
</table>
<table name="host_metrics">
<row ref="OpaqueRef:hm1" memory__total="34359738368" memory__free="8589934592" last_updated="20260615T12:00:00Z"/>
<row ref="OpaqueRef:hm2" memory__total="34359738368" memory__free="12884901888"/>
</table>
<table name="SR">
<row ref="OpaqueRef:sr-local" uuid="sr-uuid-local" name__label="Local storage" type="lvm" shared="false" physical_size="998999399104" physical_utilisation="123456789" PBDs="('OpaqueRef:pbd1' 'OpaqueRef:pbd2')" VDIs="('OpaqueRef:vdi2')"/>
<row ref="OpaqueRef:sr-nfs" uuid="sr-uuid-nfs" name__label="NFS repo" type="nfs" shared="true" physical_size="2199023255552" physical_utilisation="1099511627776" PBDs="('OpaqueRef:pbd3' 'OpaqueRef:pbd4')" VDIs="('OpaqueRef:vdi1')" other_config="(('auto-scan-interval' '300'))"/>
</table>
<table name="PBD">
<row ref="OpaqueRef:pbd1" host="OpaqueRef:host1" SR="OpaqueRef:sr-local" currently_attached="true"/>
<row ref="OpaqueRef:pbd2" host="OpaqueRef:host2" SR="OpaqueRef:sr-local" currently_attached="true"/>
<row ref="OpaqueRef:pbd3" host="OpaqueRef:host1" SR="OpaqueRef:sr-nfs" currently_attached="true"/>
<row ref="OpaqueRef:pbd4" host="OpaqueRef:host2" SR="OpaqueRef:sr-nfs" currently_attached="true"/>
</table>
<table name="VDI">
<row ref="OpaqueRef:vdi1" uuid="vdi-uuid-1" name__label="vm1-disk" type="user" virtual_size="21474836480" SR="OpaqueRef:sr-nfs" VBDs="('OpaqueRef:vbd1')"/>
<row ref="OpaqueRef:vdi2" uuid="vdi-uuid-2" name__label="vm2-disk" type="user" virtual_size="10737418240" SR="OpaqueRef:sr-local" VBDs="('OpaqueRef:vbd2')"/>
</table>
<table name="VBD">
<row ref="OpaqueRef:vbd1" uuid="vbd-uuid-1" VDI="OpaqueRef:vdi1" VM="OpaqueRef:vm1" device="xvda" type="Disk" mode="RW" bootable="true" currently_attached="true"/>
<row ref="OpaqueRef:vbd2" uuid="vbd-uuid-2" VDI="OpaqueRef:vdi2" VM="OpaqueRef:vm2" device="xvda" type="Disk" mode="RW" bootable="true" currently_attached="false"/>
</table>
<table name="VM">
<row ref="OpaqueRef:vm1" uuid="vm-uuid-1" name__label="vm1" power_state="Running" is_a_template="false" is_control_domain="false" resident_on="OpaqueRef:host1" memory__dynamic_max="4294967296" VCPUs__max="4" VIFs="('OpaqueRef:vif1')" VBDs="('OpaqueRef:vbd1')" guest_metrics="OpaqueRef:gm1"/>
<row ref="OpaqueRef:vm2" uuid="vm-uuid-2" name__label="vm2" power_state="Halted" is_a_template="false" is_control_domain="false" resident_on="OpaqueRef:NULL" memory__dynamic_max="2147483648" VCPUs__max="2" VIFs="()" VBDs="('OpaqueRef:vbd2')" guest_metrics="OpaqueRef:NULL"/>
<row ref="OpaqueRef:dom0" uuid="dom0-uuid" name__label="Control domain on host1" power_state="Running" is_a_template="false" is_control_domain="true" domid="0" resident_on="OpaqueRef:host1" affinity="OpaqueRef:host1" memory__dynamic_max="4294967296" memory__target="4294967296" VCPUs__max="4" VIFs="()" VBDs="()" guest_metrics="OpaqueRef:NULL"/>
<row ref="OpaqueRef:tmpl" uuid="vm-uuid-tpl" name__label="Debian%.12%.template" power_state="Halted" is_a_template="true" is_control_domain="false" resident_on="OpaqueRef:NULL" memory__dynamic_max="2147483648" VCPUs__max="2" VIFs="()" VBDs="()" guest_metrics="OpaqueRef:NULL"/>
</table>
<table name="VM_guest_metrics">
<row ref="OpaqueRef:gm1" os_version="(('name' 'Debian%.12'))"/>
</table>
<table name="VIF">
<row ref="OpaqueRef:vif1" uuid="vif-uuid-1" VM="OpaqueRef:vm1" network="OpaqueRef:net0" device="0" MAC="aa:bb:cc:dd:ee:01" currently_attached="true"/>
</table>
<table name="PIF">
<row ref="OpaqueRef:pif1" uuid="pif-uuid-1" host="OpaqueRef:host1" network="OpaqueRef:net0" device="eth0" MAC="aa:bb:cc:dd:ee:f1" IP="192.168.1.11" currently_attached="true" VLAN="-1" management="true"/>
</table>
<table name="network">
<row ref="OpaqueRef:net0" uuid="net-uuid-0" name__label="Pool-wide%.network%.associated%.with%.eth0"/>
</table>
</database>
"""

INVENTORY = """# automatically generated
INSTALLATION_UUID='inv-uuid'
PRODUCT_VERSION='8.3.0'
PRODUCT_VERSION_TEXT='8.3.0'
XEN_VERSION='4.17'
KERNEL_VERSION='4.19.0-1.el8.xcpng'
INSTALLATION_UUID='host-uuid-1'
HOSTNAME='host1.example.com'
CONTROL_DOMAIN_MEMORY='2048'
CONTROL_DOMAIN_CPUS='2'
"""


def build(host: str, suffix: str):
    os.makedirs(OUT, exist_ok=True)
    base = f"{host}/etc/xensource-inventory"
    if suffix == "tar.bz2":
        path = os.path.join(OUT, f"{host}.tar.bz2")
        with tarfile.open(path, "w:bz2") as tar:
            for name, data in [
                (f"{host}/xapi-db.xml", XAPI_DB),
                (base, INVENTORY),
            ]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                tar.addfile(info, io.BytesIO(data.encode()))
    else:
        path = os.path.join(OUT, f"{host}.zip")
        with zipfile.ZipFile(path, "w") as z:
            z.writestr(f"{host}/xapi-db.xml", XAPI_DB)
            z.writestr(base, INVENTORY)
    print("built", path)


if __name__ == "__main__":
    # also write a plain xapi-db.xml for rust unit tests
    with open(os.path.join(OUT, "xapi-db.xml"), "w") as f:
        f.write(XAPI_DB)
    for host in ("host1", "host2"):
        build(host, "tar.bz2")
    build("host1", "zip")
