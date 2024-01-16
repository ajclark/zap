# zap
Zap is designed to transmit a single file over a high-latency fat internet pipe as quickly as possible. e.g. California to New York or London to Sydney. 

## How does Zap work?
Zap splits a single file in to 'chunks' and copies all chunks in parallel via SSH. This creates multiple parallel network flows that increases the aggregate utilization of the network pipe. Zap does not use any additional local disk space when creating chunks, instead Zap reads the input file at different offsets in parallel and streams these offsets directly across the network via SSH. This saves time and avoids wasting local disk space. To send a 100GB file, Zap requires 200GB of remote space but no additional local space. Zap uses the additional remote space to write out the temporary chunks prior to final assembly of the file. 

Zap also takes advantage of the BBR TCP congestion control algorithm, which achieves higher overall TCP throughput over high-RTT links than CUBIC.

## Why would I want this?
Good use cases for Zap might be sending a large video file to someone on the other side of the globe as fast as possible. 

## What if I have multiple files to send across a high-RTT link?
If you need to send multiple files then rclone or rsync is likely better suited. Note that to drive up the utilization of your network pipe you will have to use rsync in conjunction with xargs or GNU parallel.

## Does Zap help on low-RTT links?
Yes. Take a look at the benchmarks below. A single file copy with scp might max out at 4Gbps on a local 10G LAN, where as Zap can drive 2x the throughput thanks to parallelism.  

## Benchmarks
``` 
LAN (10Gbps pipe)
- scp: 4000 Mbps
- zap: 8497 Mbps

WAN (1Gbps pipe; tailscale; 70ms RTT)
- scp: 100-300 Mbps
- zap: 700-850 Mbps
```
