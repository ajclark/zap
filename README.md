# zap
Zap is designed to transmit a file as fast as possible over a high-latency, high-bandwidth network connection. e.g. California to New York, London to Sydney, or within datacenters. Zap is many times faster than conventional file transfer tools and aims to saturate the network connection, even over high-latency links. Zap has been tested with 100 Gbps NICs in both datacenter and WAN scenarios.

## Demo
<img src="https://github.com/ajclark/zap/blob/main/zap.gif?raw=true">

## Example
Pull a file from remote to local:
`zap user@remote_host:/path/to/remote_file /local/destination/`

Push a file from local to remote:
`zap /local/path/to/file user@remote_host:/remote/destination/`

## Usage
```
Zap — Fast file copy 0.1
Allan Clark. <napta2k@gmail.com>
Transfers a file in parallel streams over SSH

USAGE:
    zap [OPTIONS] <source> <destination>

ARGS:
    <source>         Source file (local file or user@host:remote_path)
    <destination>    Destination (local file or user@host:remote_path)

OPTIONS:
    -h, --help                           Print help information
    -i, --ssh-key-path <ssh_key_path>    The SSH key path for authentication
    -p, --port <port>                    SSH port [default: 22]
    -r, --retries <retries>              The number of retries to attempt [default: 3]
    -s, --streams <streams>              The number of parallel streams [default: 20]
    -V, --version                        Print version information

EXAMPLES:
	Pull a file from remote to local:
		zap user@remote_host:/path/to/remote_file /local/destination/

	Push a file from local to remote:
		zap /local/path/to/file user@remote_host:/remote/destination/
```

## How does Zap work?
Zap splits a single file in to 'streams' and copies all streams in parallel via SSH. This creates multiple parallel network flows that increases the aggregate utilization of the network pipe. Zap does not use any additional local disk space when creating streams, instead Zap reads the input file at different offsets in parallel and streams these offsets directly across the network via SSH. This saves time and avoids wasting local disk space. 

Zap also takes advantage of the BBR TCP congestion control algorithm, which achieves higher overall TCP throughput over high latency links than CUBIC.

## Recommended OS settings
For the fastest possible throughput on high latency links, change the congestion control algorithm on the sender side to BBR: `sysctl net.ipv4.tcp_congestion_control=bbr`. Make this permanent through updating `/etc/sysctl.conf`.

## FAQ
### Why would I want this?
You should consider Zap if your existing file transfer tool is not adequately utilizing your available network bandwidth.

### What if I have multiple files to send across a high latency link?
If you need to send multiple files then rclone or rsync is likely better suited. Note that to drive up the utilization of your network pipe with rsync xargs or GNU parallel is likely required. It is also possible to run multiple instances of zap as you would with any other command. e.g. xargs -P.

### Does Zap help on low latency links?
Yes. Take a look at the benchmarks below. A single file copy with scp might max out at 7 Gbps on a local 100G LAN, where as Zap can drive 40-80 Gbps throughput.

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

1. Install rust
2. `cd zap && cargo build --release`
