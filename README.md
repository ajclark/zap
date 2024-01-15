# zap
Zap is designed to be fast at copying a single file over a fat network pipe with high RTT latency. e.g. California to New York or London to Sydney. 

## How does it work?
Zap splits a single file in to chunks and copies all chunks in parallel via SSH. This creates multiple parallel network flows that increase the aggregate utilization of the network pipe. Zap also takes advantage of the BBR TCP congestion control algorithm, which achieves higher overall TCP throughput over high-RTT links than CUBIC. Zap can initially overwhelm the remote target's SSH daemon and automatically retries failed chunks. 

Zap reads the input file at different offsets in parallel. This saves time and avoids wasting local disk space through creating chunks on the local side. However, Zap sends these chunks to the remote side and then assembles the file, requiring N+1 disk space on the remote side. To send a 100GB file would require 200GB of storage on the remote side, including the temporary chunks + final assembled file. 

## Why would I want this?
Good use cases for Zap might be sending large video files to people on the other side of the globe as fast as possible. 

## Benchmarks
``` 
LAN (10Gbps pipe)
- scp: 4000 Mbps
- zap.py: 8497 Mbps

WAN (1Gbps pipe; tailscale; 70ms RTT)
- scp: 100-300 Mbps
- zap.py: 700-850 Mbps
```