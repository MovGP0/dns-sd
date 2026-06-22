use clap::{ArgAction, Parser};
use mdns_sd::{HostnameResolutionEvent, ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent};
use std::collections::HashSet;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_DOMAIN: &str = "local.";
const DEFAULT_TIMEOUT_SECONDS: u64 = 0;

#[derive(Debug, Parser)]
#[command(
    name = "dns-sd",
    about = "DNS-SD/mDNS discovery browser with Apple dns-sd style flags.",
    disable_help_subcommand = true,
    disable_help_flag = true,
    arg_required_else_help = true,
    override_usage = "dns-sd [flags]\n  dns-sd [command flags]",
    help_template = "{about}\n\nUsage:\n  {usage}\n\n{all-args}\n\nExamples:\n  dns-sd --types\n  dns-sd --browse _http._tcp\n  dns-sd --locate \"My Printer\" _ipp._tcp local.\n  dns-sd --query my-host.local. A --timeout 5\n"
)]
struct Args
{
    #[arg(
        short = 'b',
        long = "browse",
        short_alias = 'B',
        value_names = ["type", "domain"],
        num_args = 1..=2,
        help = "Browse service instances (domain defaults to local.)",
        help_heading = "Discovery Commands"
    )]
    browse: Option<Vec<String>>,

    #[arg(
        short = 'l',
        long = "locate",
        short_alias = 'L',
        value_names = ["name", "type", "domain"],
        num_args = 2..=3,
        help = "Resolve a service instance (domain defaults to local.)",
        help_heading = "Discovery Commands"
    )]
    lookup: Option<Vec<String>>,

    #[arg(
        short = 'q',
        long = "query",
        short_alias = 'Q',
        value_names = ["fqdn", "rrtype", "rrclass"],
        num_args = 2..=3,
        help = "Query host address records",
        help_heading = "Discovery Commands"
    )]
    query: Option<Vec<String>>,

    #[arg(
        short = 't',
        long = "types",
        short_alias = 'Z',
        help = "Browse advertised DNS-SD service types",
        help_heading = "Discovery Commands"
    )]
    browse_types: bool,

    #[arg(
        long,
        value_name = "seconds",
        default_value_t = DEFAULT_TIMEOUT_SECONDS,
        help = "Stop after this many seconds; 0 runs until interrupted",
        help_heading = "Flags"
    )]
    timeout: u64,

    #[arg(
        short = 'h',
        long = "help",
        action = ArgAction::Help,
        help = "help for dns-sd",
        help_heading = "Flags"
    )]
    help: Option<bool>,
}

#[derive(Debug)]
enum Command
{
    Browse
    {
        service_type: String,
    },
    Lookup
    {
        instance_name: String,
        service_type: String,
    },
    Query
    {
        fqdn: String,
        rr_type: QueryRecordType,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueryRecordType
{
    A,
    Aaaa,
    AnyAddress,
}

fn main() -> ExitCode
{
    match run()
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) =>
        {
            eprintln!("dns-sd: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String>
{
    let args = Args::parse();
    let command = parse_command(&args)?;
    let mdns = ServiceDaemon::new().map_err(|error| format!("failed to create mDNS daemon: {error}"))?;
    let result = match command
    {
        Command::Browse { service_type } => browse(&mdns, &service_type, args.timeout),
        Command::Lookup {
            instance_name,
            service_type,
        } => lookup(&mdns, &instance_name, &service_type, args.timeout),
        Command::Query { fqdn, rr_type } => query_hostname(&mdns, &fqdn, rr_type, args.timeout),
    };

    let shutdown_result = mdns
        .shutdown()
        .map(|_| ())
        .map_err(|error| format!("failed to shut down mDNS daemon: {error}"));
    result.and(shutdown_result)
}

fn parse_command(args: &Args) -> Result<Command, String>
{
    let selected_count = args.browse.is_some() as u8
        + args.lookup.is_some() as u8
        + args.query.is_some() as u8
        + args.browse_types as u8;

    if selected_count != 1
    {
        return Err("select exactly one of -B, -L, -Q, or -Z".to_string());
    }

    if args.browse_types
    {
        return Ok(Command::Browse {
            service_type: "_services._dns-sd._udp.local.".to_string(),
        });
    }

    if let Some(values) = &args.browse
    {
        let service_type = normalize_service_type(&values[0], values.get(1).map(String::as_str))?;
        return Ok(Command::Browse { service_type });
    }

    if let Some(values) = &args.lookup
    {
        let service_type = normalize_service_type(&values[1], values.get(2).map(String::as_str))?;
        return Ok(Command::Lookup {
            instance_name: values[0].clone(),
            service_type,
        });
    }

    if let Some(values) = &args.query
    {
        let rr_type = parse_query_record_type(&values[1])?;
        validate_query_class(values.get(2))?;
        return Ok(Command::Query {
            fqdn: normalize_hostname(&values[0]),
            rr_type,
        });
    }

    Err("no command selected".to_string())
}

fn browse(mdns: &ServiceDaemon, service_type: &str, timeout_seconds: u64) -> Result<(), String>
{
    let receiver = mdns
        .browse(service_type)
        .map_err(|error| format!("failed to browse {service_type}: {error}"))?;

    println!("Browsing for {service_type}");
    print_browse_header();

    let started = Instant::now();
    while !has_timed_out(started, timeout_seconds)
    {
        match receiver.recv_timeout(Duration::from_millis(500))
        {
            Ok(event) => print_service_event(service_type, None, event),
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn lookup(
    mdns: &ServiceDaemon,
    instance_name: &str,
    service_type: &str,
    timeout_seconds: u64,
) -> Result<(), String>
{
    let receiver = mdns
        .browse(service_type)
        .map_err(|error| format!("failed to browse {service_type}: {error}"))?;

    println!("Lookup {instance_name}.{service_type}");
    print_browse_header();

    let started = Instant::now();
    let expected_fullname = format!("{}.{service_type}", instance_name.trim_end_matches('.'));
    while !has_timed_out(started, timeout_seconds)
    {
        match receiver.recv_timeout(Duration::from_millis(500))
        {
            Ok(event) => print_service_event(service_type, Some(&expected_fullname), event),
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn query_hostname(
    mdns: &ServiceDaemon,
    hostname: &str,
    rr_type: QueryRecordType,
    timeout_seconds: u64,
) -> Result<(), String>
{
    let timeout_millis = match timeout_seconds
    {
        0 => None,
        seconds => Some(seconds * 1000),
    };
    let receiver = mdns
        .resolve_hostname(hostname, timeout_millis)
        .map_err(|error| format!("failed to query {hostname}: {error}"))?;

    println!("Querying {hostname} {rr_type}");
    println!("{:<12} {:<6} {:<6} {:<39} Hostname", "Timestamp", "A/R", "Type", "Address");

    let started = Instant::now();
    while !has_timed_out(started, timeout_seconds)
    {
        match receiver.recv_timeout(Duration::from_millis(500))
        {
            Ok(HostnameResolutionEvent::AddressesFound(found_hostname, addresses)) =>
            {
                print_addresses("Add", rr_type, &found_hostname, &addresses);
            }
            Ok(HostnameResolutionEvent::AddressesRemoved(found_hostname, addresses)) =>
            {
                print_addresses("Rmv", rr_type, &found_hostname, &addresses);
            }
            Ok(HostnameResolutionEvent::SearchTimeout(_)) | Ok(HostnameResolutionEvent::SearchStopped(_)) => break,
            Ok(HostnameResolutionEvent::SearchStarted(_)) => {}
            Ok(_) => {}
            Err(flume::RecvTimeoutError::Timeout) => {}
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

fn print_service_event(service_type: &str, expected_fullname: Option<&str>, event: ServiceEvent)
{
    match event
    {
        ServiceEvent::ServiceFound(_, fullname) if matches_fullname(expected_fullname, &fullname) =>
        {
            print_browse_line("Add", service_type, &fullname);
        }
        ServiceEvent::ServiceRemoved(_, fullname) if matches_fullname(expected_fullname, &fullname) =>
        {
            print_browse_line("Rmv", service_type, &fullname);
        }
        ServiceEvent::ServiceResolved(service) if matches_fullname(expected_fullname, service.get_fullname()) =>
        {
            print_resolved_service(&service);
        }
        _ => {}
    }
}

fn print_browse_header()
{
    println!(
        "{:<12} {:<6} {:<8} {:<24} Instance Name",
        "Timestamp", "A/R", "Domain", "Service Type"
    );
}

fn print_browse_line(action: &str, service_type: &str, fullname: &str)
{
    println!(
        "{:<12} {:<6} {:<8} {:<24} {}",
        timestamp_seconds(),
        action,
        domain_from_service_type(service_type),
        service_type_without_domain(service_type),
        instance_name_from_fullname(fullname, service_type)
    );
}

fn print_resolved_service(service: &ResolvedService)
{
    println!(
        "{:<12} {:<6} {:<8} {:<24} {}",
        timestamp_seconds(),
        "Res",
        domain_from_service_type(&service.ty_domain),
        service_type_without_domain(&service.ty_domain),
        instance_name_from_fullname(service.get_fullname(), &service.ty_domain)
    );
    println!("    host: {}", service.get_hostname());
    println!("    port: {}", service.get_port());

    let mut addresses = service.get_addresses().iter().map(ToString::to_string).collect::<Vec<_>>();
    addresses.sort();
    for address in addresses
    {
        println!("    address: {address}");
    }

    let mut properties = service.get_properties().iter().map(ToString::to_string).collect::<Vec<_>>();
    properties.sort();
    for property in properties
    {
        println!("    txt: {property}");
    }
}

fn print_addresses(action: &str, rr_type: QueryRecordType, hostname: &str, addresses: &HashSet<ScopedIp>)
{
    let mut addresses = addresses
        .iter()
        .filter(|address| match (rr_type, address)
        {
            (QueryRecordType::A, ScopedIp::V4(_)) => true,
            (QueryRecordType::Aaaa, ScopedIp::V6(_)) => true,
            (QueryRecordType::AnyAddress, _) => true,
            _ => false,
        })
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    addresses.sort();

    for address in addresses
    {
        println!(
            "{:<12} {:<6} {:<6} {:<39} {}",
            timestamp_seconds(),
            action,
            rr_type.label(),
            address,
            hostname
        );
    }
}

fn normalize_service_type(service_type: &str, domain: Option<&str>) -> Result<String, String>
{
    let service_type = service_type.trim_end_matches('.');
    if !service_type.starts_with('_') || !(service_type.contains("._tcp") || service_type.contains("._udp"))
    {
        return Err(format!("{service_type} is not a DNS-SD service type like _http._tcp"));
    }

    let domain = domain.map(normalize_domain).unwrap_or_else(|| DEFAULT_DOMAIN.to_string());
    if service_type.ends_with("._tcp") || service_type.ends_with("._udp")
    {
        Ok(format!("{service_type}.{domain}"))
    }
    else
    {
        Ok(format!("{service_type}."))
    }
}

fn normalize_domain(domain: &str) -> String
{
    let trimmed = domain.trim_matches('.');
    if trimmed.is_empty()
    {
        DEFAULT_DOMAIN.to_string()
    }
    else
    {
        format!("{trimmed}.")
    }
}

fn normalize_hostname(hostname: &str) -> String
{
    if hostname.ends_with('.')
    {
        hostname.to_string()
    }
    else
    {
        format!("{hostname}.")
    }
}

fn parse_query_record_type(value: &str) -> Result<QueryRecordType, String>
{
    match value.to_ascii_uppercase().as_str()
    {
        "A" => Ok(QueryRecordType::A),
        "AAAA" => Ok(QueryRecordType::Aaaa),
        "ADDR" | "ADDRESS" | "ANY" => Ok(QueryRecordType::AnyAddress),
        unsupported => Err(format!("unsupported -Q record type {unsupported}; supported: A, AAAA, ADDR")),
    }
}

fn validate_query_class(value: Option<&String>) -> Result<(), String>
{
    match value.map(|text| text.to_ascii_uppercase())
    {
        None => Ok(()),
        Some(class) if class == "IN" || class == "1" => Ok(()),
        Some(class) => Err(format!("unsupported -Q record class {class}; only IN is supported")),
    }
}

fn matches_fullname(expected_fullname: Option<&str>, actual_fullname: &str) -> bool
{
    expected_fullname
        .map(|expected| expected.eq_ignore_ascii_case(actual_fullname))
        .unwrap_or(true)
}

fn has_timed_out(started: Instant, timeout_seconds: u64) -> bool
{
    timeout_seconds > 0 && started.elapsed() >= Duration::from_secs(timeout_seconds)
}

fn timestamp_seconds() -> String
{
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    let hours = timestamp / 3_600;
    let minutes = (timestamp % 3_600) / 60;
    let seconds = timestamp % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

fn domain_from_service_type(service_type: &str) -> &str
{
    service_type
        .strip_prefix(service_type_without_domain(service_type))
        .and_then(|value| value.strip_prefix('.'))
        .unwrap_or(DEFAULT_DOMAIN)
}

fn service_type_without_domain(service_type: &str) -> &str
{
    if let Some(index) = service_type.find("._tcp.")
    {
        &service_type[..index + "._tcp".len()]
    }
    else if let Some(index) = service_type.find("._udp.")
    {
        &service_type[..index + "._udp".len()]
    }
    else
    {
        service_type
    }
}

fn instance_name_from_fullname<'a>(fullname: &'a str, service_type: &str) -> &'a str
{
    fullname
        .strip_suffix(service_type)
        .and_then(|value| value.strip_suffix('.'))
        .unwrap_or(fullname)
}

impl QueryRecordType
{
    fn label(self) -> &'static str
    {
        match self
        {
            Self::A => "A",
            Self::Aaaa => "AAAA",
            Self::AnyAddress => "ADDR",
        }
    }
}

impl std::fmt::Display for QueryRecordType
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        formatter.write_str(self.label())
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn normalizes_service_type_with_default_domain()
    {
        assert_eq!(
            normalize_service_type("_http._tcp", None).unwrap(),
            "_http._tcp.local."
        );
    }

    #[test]
    fn normalizes_service_type_with_explicit_domain()
    {
        assert_eq!(
            normalize_service_type("_ipp._tcp", Some("local.")).unwrap(),
            "_ipp._tcp.local."
        );
    }

    #[test]
    fn extracts_instance_name()
    {
        assert_eq!(
            instance_name_from_fullname("Printer._ipp._tcp.local.", "_ipp._tcp.local."),
            "Printer"
        );
    }

    #[test]
    fn rejects_unsupported_query_class()
    {
        assert!(validate_query_class(Some(&"CH".to_string())).is_err());
    }
}
