# zap
Zap is designed to transmit a single file over a high-latency, high-bandwidth internet connection as quickly as possible. e.g. California to New York or London to Sydney. Zap is 6-8X faster than conventional file transfer tools and has been tested with 100 Gbps NICs. Zap's goal is to saturate the network interface, even over high-latency links.

## How does Zap work?
Zap splits a single file in to 'streams' and copies all streams in parallel via SSH. This creates multiple parallel network flows that increases the aggregate utilization of the network pipe. Zap does not use any additional local disk space when creating streams, instead Zap reads the input file at different offsets in parallel and streams these offsets directly across the network via SSH. This saves time and avoids wasting local disk space. 

To send a 100GB file, Zap requires 105GB of total remote space but no additional local space. Zap uses the additional remote space as buffer write out the temporary streams prior to final assembly of the file. The remote end disk space formula is `filesize + size_of_a_stream`.

Zap also takes advantage of the BBR TCP congestion control algorithm, which achieves higher overall TCP throughput over high-RTT links than CUBIC.

## Requirements
For fastest throughput on high-RTT links, change the congestion algorithm on both ends to BBR: `sysctl net.ipv4.tcp_congestion_control=bbr`. Make this permanent through updating `/etc/sysctl.conf`

## Usage
```
USAGE:
    zap [OPTIONS] <input_file> <user@host:remote_path>

ARGS:
    <input_file>               The input file path
    <user@host:remote_path>    Specifies user@host:remote_path

OPTIONS:
    -c, --streams <streams>              The number of parallel streams [default: 20]
    -h, --help                           Print help information
    -i, --ssh-key-path <ssh_key_path>    The SSH key path for authentication
    -p, --port <port>                    SSH port [default: 22]
    -r, --retries <retries>              The number of retries to attempt [default: 3]
    -V, --version                        Print version information
```

`./zap 100mb.bin user@host:`

### Why would I want this?
Good use cases for Zap might be sending a large video file to someone on the other side of the country or globe as fast as possible. 

### What if I have multiple files to send across a high-RTT link?
If you need to send multiple files then rclone or rsync is likely better suited. Note that to drive up the utilization of your network pipe you will have to use rsync in conjunction with xargs or GNU parallel. It is also possible to run multiple instances of zap as you would with any other command. e.g. xargs. 

### Does Zap help on low-RTT links?
Yes. Take a look at the benchmarks below. A single file copy with scp might max out at 4Gbps on a local 10G LAN, where as Zap can drive 2x the throughput thanks to parallelism.  

## Benchmarks
``` 
LAN (10 Gbps)
- scp: 4000 Mbps
- zap: 8497 Mbps

LAN (100 Gbps)
- scp: 6800 Mbps
- zap: 40000 Mbps
- zap: 80000 Mbps (100 streams)

WAN (1 Gbps; tailscale; 70ms RTT)
- scp: 100-300 Mbps
- zap: 700-850 Mbps

WAN (40 Gbps)*
- zap: San Jose <> Tokyo: 3000 Mbps
- zap: San Jose <> Tokyo: 4000 Mbps (50 streams)
- zap: San Jose <> Tokyo: 12000 Mbps (100 streams)
```
*Note: 40Gbps San Jose testing instances are 40Gbps network connections but unknown bottlenecks between datacenters. Also maxed out disk bandwidth and heavy SSH CPU contention at these speeds.

## Build instructions
Zap has been tested On Debian/Ubuntu/EL Linux and MacOS. It will possibly work on Windows, but requires SSH and a shell environment.

`cd zap && cargo build --release`

