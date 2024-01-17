# zap
Zap is designed to transmit a single file over a high-latency, high-bandwidth internet connection as quickly as possible. e.g. California to New York or London to Sydney. 

## How does Zap work?
Zap splits a single file in to 'streams' and copies all streams in parallel via SSH. This creates multiple parallel network flows that increases the aggregate utilization of the network pipe. Zap does not use any additional local disk space when creating streams, instead Zap reads the input file at different offsets in parallel and streams these offsets directly across the network via SSH. This saves time and avoids wasting local disk space. To send a 100GB file, Zap requires 200GB of remote space but no additional local space. Zap uses the additional remote space to write out the temporary streams prior to final assembly of the file. 

Zap also takes advantage of the BBR TCP congestion control algorithm, which achieves higher overall TCP throughput over high-RTT links than CUBIC.

## Requirements
For fastest throughput on high-RTT links, change the congestion algorithm on both ends to BBR: `sysctl net.ipv4.tcp_congestion_control=bbr`. Make this permanent through updating `/etc/sysctl.conf`

## Usage
```
USAGE:
    zap [OPTIONS] --user <user> --server <server> --remote-path <remote_path> <input_file>

ARGS:
    <input_file>    The input file path

OPTIONS:
    -c, --streams <streams>              The number of parallel streams [default: 20]
    -h, --help                           Print help information
    -i, --ssh-key-path <ssh_key_path>    The SSH key path for authentication
    -p, --remote-path <remote_path>      The remote path where streams will be stored
    -r, --retries <retries>              The number of retries to attempt [default: 3]
    -s, --server <server>                The hostname of the remote server
    -u, --user <user>                    The username for the remote server
    -V, --version                        Print version information
```

`./zap -u ubuntu -s 1.2.3.4 -p /home/ubuntu my-file.bin`

### Why would I want this?
Good use cases for Zap might be sending a large video file to someone on the other side of the globe as fast as possible. 

### What if I have multiple files to send across a high-RTT link?
If you need to send multiple files then rclone or rsync is likely better suited. Note that to drive up the utilization of your network pipe you will have to use rsync in conjunction with xargs or GNU parallel.

### Does Zap help on low-RTT links?
Yes. Take a look at the benchmarks below. A single file copy with scp might max out at 4Gbps on a local 10G LAN, where as Zap can drive 2x the throughput thanks to parallelism.  

## Benchmarks
``` 
LAN (10Gbps pipe)
- scp: 4000 Mbps
- zap: 8497 Mbps

WAN (1Gbps pipe; tailscale; 70ms RTT)
- scp: 100-300 Mbps
- zap: 700-850 Mbps

WAN (40Gbps pipe)*
- zap: San Jose <> Tokyo: 3000 Mbps
- zap: San Jose <> Tokyo: 4000 Mbps (50 streams)
- zap: San Jose <> Tokyo: 120000 Mbps (100 streams)
```
*Note: 40Gbps San Jose testing instances are 40Gbps network connections but unknown bottlenecks between datacenters. Also maxed out disk bandwidth and heavy SSH CPU contention at these speeds.

## Build instructions
Zap has been tested On Debian/Ubuntu/RHEL Linux and MacOS. It will likely work on Windows, but depends on SSH.
`cd zap && cargo build --release`

