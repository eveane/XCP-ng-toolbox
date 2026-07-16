# Collection of scripts and tools for XCP-ng
Thesee tools have been developped do address adhoc situation i have encountered. 
They mey not apply to generic situation, and haven't been tested outside the specific situation i have encountered.

yet they may help others or actually apply in wider situation.

please use them with a pinch of slat and after careful review.

## Bond Create

script to create bond. 
incomplete.

## Bugtool collector

work in proghress. missing some dependancy to run properly. 
it collect all the xen-bugtool from a pool and save them on an archive on xoa.

## v2v vm disk progress

### How to Use

Save the script as vm_disk_progress.sh.
Make it executable:

`chmod +x vm_disk_progress.sh`

Run it with the VM name:

`sudo ./vm_disk_progress.sh myVm`


### Expected Output
For your logs, the output will now look like this:

```
disk associated :
  myVm/myVm-000001.vmdk
  Log path: /tmp/xo-serverN4Xjyc
  Total blocks to transfer: 214726344703
  Latest offset: 212766752768
  Progress: 99.09%
---
  myVm/myVm1_1-000001.vmdk
  Log path: /tmp/xo-serverTT2vuI
  Total blocks to transfer: 3089733255167
  Latest offset: 3295888539648
  Progress: 100.00%
---
  myVm/myVm1_2-000001.vmdk
  Log path: /tmp/xo-serverpojgvB
  Total blocks to transfer: 536083890175
  Latest offset: 535898882048
  Progress: 99.97%
---
```








