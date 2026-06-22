# dns-sd

Apple `dns-sd`-style DNS-SD/mDNS discovery browser written in Rust.

This utility is meant for quickly inspecting services advertised on the local
network. It uses multicast DNS, so it works best for `.local.` names and
DNS-SD service types such as `_http._tcp`, `_ipp._tcp`, and `_ssh._tcp`.

## Features

- Browse service instances by service type with `--browse` / `-b`.
- Browse advertised service types with `--types` / `-t`.
- Resolve a named service instance with `--locate` / `-l`.
- Query `.local.` host addresses with `--query` / `-q`.
- Print resolved host, port, address, and TXT record details.
- Run until interrupted, or stop automatically with `--timeout`.

## Install

Build from source with Cargo:

```powershell
git clone https://github.com/<owner>/dns-sd.git
cd dns-sd
cargo build --release
```

The binary is written to:

```text
target/release/dns-sd.exe
```

On Linux and macOS the binary path is:

```text
target/release/dns-sd
```

## Usage

```text
DNS-SD/mDNS discovery browser with Apple dns-sd style flags.

Usage:
  dns-sd [flags]
  dns-sd [command flags]

Discovery Commands:
  -b, -B, --browse <type> <domain>            Browse service instances (domain defaults to local.)
  -l, -L, --locate <name> <type> <domain>     Resolve a service instance (domain defaults to local.)
  -q, -Q, --query <fqdn> <rrtype> <rrclass>   Query host address records
  -t, -Z, --types                             Browse advertised DNS-SD service types

Flags:
      --timeout <seconds>  Stop after this many seconds; 0 runs until interrupted [default: 0]
  -h, --help               help for dns-sd
```

`DOMAIN` defaults to `local.` when omitted. `RRCLASS` defaults to `IN` when
omitted.

## Examples

Browse advertised service types:

```powershell
cargo run -- --types
```

Browse HTTP services:

```powershell
cargo run -- --browse _http._tcp
```

Browse IPP printers in the local domain:

```powershell
cargo run -- --browse _ipp._tcp local.
```

Resolve a specific printer:

```powershell
cargo run -- --locate "My Printer" _ipp._tcp local.
```

Query a `.local.` host for IPv4 addresses:

```powershell
cargo run -- --query my-host.local. A --timeout 5
```

Query a `.local.` host for IPv6 addresses:

```powershell
cargo run -- --query my-host.local. AAAA --timeout 5
```

After building a release binary, replace `cargo run --` with the executable
path:

```powershell
.\target\release\dns-sd.exe --browse _http._tcp --timeout 5
```

## Output

Browsing prints Apple `dns-sd`-style event rows:

```text
Browsing for _http._tcp.local.
Timestamp    A/R    Domain   Service Type             Instance Name
07:54:28     Add    local.   _http._tcp               Example Web Server
07:54:28     Res    local.   _http._tcp               Example Web Server
    host: example.local.
    port: 80
    address: 192.168.1.10
    txt: path=/
```

`Add` means a service was discovered, `Rmv` means it was removed, and `Res`
means SRV/TXT/address data was resolved.

## Supported Commands

| Command | Status | Notes |
| --- | --- | --- |
| `--browse`, `-b`, `-B` | Supported | Browses and resolves matching service instances. |
| `--locate`, `-l`, `-L` | Supported | Browses the type and filters to the requested instance name. |
| `--query`, `-q`, `-Q` | Supported | Supports `A`, `AAAA`, and `ADDR` over mDNS. |
| `--types`, `-t`, `-Z` | Supported | Browses `_services._dns-sd._udp.local.`. |
| `-R` | Not implemented | Service registration is outside the current browser/discovery scope. |

## Notes

- Multicast DNS traffic can be blocked by firewalls, VPNs, VLAN boundaries, or
  Wi-Fi client isolation.
- The current implementation focuses on multicast DNS-SD. It does not perform
  unicast DNS queries.
- `--query` is limited to host address lookups exposed by the underlying mDNS
  library.

## Development

Run the tests:

```powershell
cargo test
```

Run a bounded local smoke test:

```powershell
cargo run -- --types --timeout 1
```

The implementation uses [`mdns-sd`](https://crates.io/crates/mdns-sd) for the
mDNS/DNS-SD protocol work and [`clap`](https://crates.io/crates/clap) for CLI
argument parsing.
