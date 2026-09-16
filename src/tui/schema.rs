//! Field descriptions for sing-box 1.14 objects. The editor shows common
//! fields first and every documented field on request. Fields not listed here
//! are still shown and kept verbatim, so newer core options are never lost.
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ns {
    Outbound,
    Dns,
    RuleSet,
    Inbound,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Object {
    Inbound,
    Outbound,
    Endpoint,
    DnsServer,
    DnsRule,
    DnsOptions,
    RouteRule,
    RouteOptions,
    RuleSet,
    HeadlessRule,
    Log,
    Ntp,
    Certificate,
    Experimental,
    CacheFile,
    ClashApi,
    Service,
    Unknown,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Text,
    Secret,
    Number,
    Bool,
    Duration,
    Enum(&'static [&'static str]),
    Ref(Ns),
    Refs(Ns),
    /// Array of strings (or numbers where `numbers`), one per line when editing.
    List {
        numbers: bool,
    },
    Json,
    Nested(Object),
    NestedList(Object),
}
#[derive(Clone, Debug)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub common: bool,
    pub help: &'static str,
}
const fn f(
    key: &'static str,
    label: &'static str,
    kind: Kind,
    common: bool,
    help: &'static str,
) -> Field {
    Field {
        key,
        label,
        kind,
        common,
        help,
    }
}
use Kind::*;
const LIST: Kind = List { numbers: false };
const NUMBERS: Kind = List { numbers: true };

impl Object {
    pub fn for_pointer(pointer: &str) -> Object {
        let parts: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
        match parts.as_slice() {
            ["inbounds", _] => Object::Inbound,
            ["outbounds", _] => Object::Outbound,
            ["endpoints", _] => Object::Endpoint,
            ["services", _] => Object::Service,
            ["dns", "servers", _] => Object::DnsServer,
            ["dns", "rules", _] => Object::DnsRule,
            ["dns"] => Object::DnsOptions,
            ["route", "rules", _] => Object::RouteRule,
            ["route", "rule_set", _] => Object::RuleSet,
            ["route"] => Object::RouteOptions,
            ["log"] => Object::Log,
            ["ntp"] => Object::Ntp,
            ["certificate"] => Object::Certificate,
            ["experimental"] => Object::Experimental,
            _ => Object::Unknown,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Object::Inbound => "Inbound",
            Object::Outbound => "Outbound",
            Object::Endpoint => "Endpoint",
            Object::DnsServer => "DNS Server",
            Object::DnsRule => "DNS Rule",
            Object::DnsOptions => "DNS",
            Object::RouteRule => "Rule",
            Object::RouteOptions => "Route",
            Object::RuleSet => "Rule Set",
            Object::HeadlessRule => "Match",
            Object::Log => "Log",
            Object::Ntp => "NTP",
            Object::Certificate => "Certificate",
            Object::Experimental => "Experimental",
            Object::CacheFile => "Cache File",
            Object::ClashApi => "Clash API",
            Object::Service => "Service",
            Object::Unknown => "Object",
        }
    }
    pub fn types(self) -> &'static [&'static str] {
        match self {
            Object::Inbound => &[
                "mixed",
                "socks",
                "http",
                "tun",
                "direct",
                "redirect",
                "tproxy",
                "shadowsocks",
                "vmess",
                "trojan",
                "vless",
                "hysteria2",
                "tuic",
                "anytls",
                "shadowtls",
                "naive",
            ],
            Object::Outbound => &[
                "direct",
                "block",
                "selector",
                "urltest",
                "socks",
                "http",
                "shadowsocks",
                "vmess",
                "trojan",
                "vless",
                "hysteria",
                "hysteria2",
                "tuic",
                "anytls",
                "shadowtls",
                "ssh",
                "tor",
                "naive",
            ],
            Object::Endpoint => &["wireguard", "tailscale"],
            Object::DnsServer => &[
                "local",
                "hosts",
                "udp",
                "tcp",
                "tls",
                "quic",
                "https",
                "h3",
                "dhcp",
                "fakeip",
                "mdns",
                "tailscale",
                "resolved",
            ],
            Object::RuleSet => &["inline", "local", "remote"],
            Object::Service => &["api", "resolved", "ssm-api", "derp", "ccm", "ocm"],
            _ => &[],
        }
    }
    pub fn tagged(self) -> bool {
        matches!(
            self,
            Object::Inbound
                | Object::Outbound
                | Object::Endpoint
                | Object::DnsServer
                | Object::RuleSet
                | Object::Service
        )
    }
}

fn dial(common_detour: bool) -> Vec<Field> {
    vec![
        f(
            "detour",
            "Via outbound",
            Ref(Ns::Outbound),
            common_detour,
            "Dial through another outbound. Empty dials directly.",
        ),
        f(
            "domain_resolver",
            "Resolve server with",
            Ref(Ns::Dns),
            false,
            "DNS server used to resolve this object's server hostname.",
        ),
        f(
            "connect_timeout",
            "Connect timeout",
            Duration,
            false,
            "e.g. 5s",
        ),
        f(
            "bind_interface",
            "Bind interface",
            Text,
            false,
            "Network interface to dial from.",
        ),
        f("inet4_bind_address", "IPv4 bind address", Text, false, ""),
        f("inet6_bind_address", "IPv6 bind address", Text, false, ""),
        f("routing_mark", "Routing mark", Number, false, "Linux only."),
        f("reuse_addr", "Reuse address", Bool, false, ""),
        f("tcp_fast_open", "TCP Fast Open", Bool, false, ""),
        f("tcp_multi_path", "Multipath TCP", Bool, false, ""),
        f("udp_fragment", "UDP fragmentation", Bool, false, ""),
        f(
            "network_strategy",
            "Network strategy",
            Enum(&["default", "hybrid", "fallback"]),
            false,
            "Graphical clients only.",
        ),
        f("fallback_delay", "Fallback delay", Duration, false, ""),
    ]
}
fn server() -> Vec<Field> {
    vec![
        f("server", "Server", Text, true, "Hostname or IP address."),
        f("server_port", "Port", Number, true, ""),
    ]
}
fn listen() -> Vec<Field> {
    vec![
        f(
            "listen",
            "Listen address",
            Text,
            true,
            "127.0.0.1 keeps it on this machine; 0.0.0.0 exposes it to your network.",
        ),
        f("listen_port", "Port", Number, true, ""),
        f("tcp_fast_open", "TCP Fast Open", Bool, false, ""),
        f("tcp_multi_path", "Multipath TCP", Bool, false, ""),
        f("udp_fragment", "UDP fragmentation", Bool, false, ""),
        f("udp_timeout", "UDP timeout", Duration, false, "e.g. 5m"),
        f("detour", "Forward to inbound", Ref(Ns::Inbound), false, ""),
    ]
}
fn tls_transport(transport: bool) -> Vec<Field> {
    let mut v = vec![f(
        "tls",
        "TLS",
        Json,
        true,
        "Outbound TLS object: enabled, server_name, insecure, utls, reality…",
    )];
    if transport {
        v.push(f(
            "transport",
            "Transport",
            Json,
            false,
            "V2Ray transport: http, ws, quic, grpc, httpupgrade.",
        ));
    }
    v.push(f("multiplex", "Multiplex", Json, false, ""));
    v
}

fn matchers(dns: bool, headless: bool) -> Vec<Field> {
    let mut v = vec![
        f("domain", "Domain", LIST, true, "Exact host names."),
        f(
            "domain_suffix",
            "Domain suffix",
            LIST,
            true,
            "example.com also matches www.example.com.",
        ),
        f(
            "domain_keyword",
            "Domain keyword",
            LIST,
            true,
            "Matches when the host contains the text.",
        ),
        f(
            "domain_regex",
            "Domain regex",
            LIST,
            true,
            "Go regular expressions, e.g. ^(.+\\.)?example\\.com$",
        ),
    ];
    if !headless {
        v.push(f(
            "rule_set",
            "Rule sets",
            Refs(Ns::RuleSet),
            true,
            "Matches when any listed rule set matches.",
        ));
    }
    if dns {
        v.push(f(
            "query_type",
            "Query type",
            LIST,
            true,
            "A, AAAA, HTTPS, … or numbers.",
        ));
    } else {
        v.push(f(
            "ip_cidr",
            "IP range",
            LIST,
            true,
            "Destination CIDR, e.g. 10.0.0.0/8.",
        ));
    }
    v.extend([
        f(
            "process_name",
            "App (process name)",
            LIST,
            true,
            "Executable name, e.g. Telegram or curl.",
        ),
        f(
            "process_path",
            "App path",
            LIST,
            !dns,
            "Full executable path.",
        ),
        f("process_path_regex", "App path regex", LIST, false, ""),
        f("port", "Port", NUMBERS, !dns, "Destination ports."),
        f("port_range", "Port range", LIST, false, "e.g. 1000:2000"),
        f(
            "network",
            "Network",
            Enum(&["tcp", "udp", "icmp"]),
            false,
            "",
        ),
    ]);
    if !headless {
        v.extend([
            f(
                "protocol",
                "Sniffed protocol",
                LIST,
                false,
                "http, tls, quic, stun, dns, bittorrent, dtls, ssh, rdp, ntp.",
            ),
            f(
                "inbound",
                "Inbound",
                Refs(Ns::Inbound),
                false,
                "Only traffic from these inbounds.",
            ),
            f("ip_version", "IP version", Enum(&["4", "6"]), false, ""),
            f("auth_user", "Authenticated user", LIST, false, ""),
            f(
                "client",
                "Sniffed client",
                LIST,
                false,
                "chromium, safari, firefox, quic-go.",
            ),
            f(
                "clash_mode",
                "Mode",
                Text,
                false,
                "Matches the Clash-style mode name.",
            ),
            f("user", "User", LIST, false, "Linux only."),
            f("user_id", "User ID", NUMBERS, false, "Linux only."),
            f(
                "rule_set_ip_cidr_match_source",
                "Rule-set IP matches source",
                Bool,
                false,
                "",
            ),
        ]);
    }
    if !dns {
        v.push(f(
            "ip_is_private",
            "Private IP",
            Bool,
            false,
            "Matches LAN and loopback destinations.",
        ));
    } else {
        v.extend([
            f(
                "ip_cidr",
                "Response IP range",
                LIST,
                false,
                "Matches the resolved address (needs match_response).",
            ),
            f("ip_is_private", "Response IP is private", Bool, false, ""),
            f("match_response", "Match response", Json, false, ""),
        ]);
    }
    v.extend([
        f("source_ip_cidr", "Source IP range", LIST, false, ""),
        f(
            "source_ip_is_private",
            "Source IP is private",
            Bool,
            false,
            "",
        ),
        f("source_port", "Source port", NUMBERS, false, ""),
        f("source_port_range", "Source port range", LIST, false, ""),
        f("package_name", "Android package", LIST, false, ""),
        f(
            "package_name_regex",
            "Android package regex",
            LIST,
            false,
            "",
        ),
        f(
            "network_type",
            "Network type",
            LIST,
            false,
            "wifi, cellular, ethernet, other.",
        ),
        f(
            "network_is_expensive",
            "Network is expensive",
            Bool,
            false,
            "",
        ),
        f(
            "network_is_constrained",
            "Network is constrained",
            Bool,
            false,
            "",
        ),
        f("wifi_ssid", "Wi-Fi SSID", LIST, false, ""),
        f("wifi_bssid", "Wi-Fi BSSID", LIST, false, ""),
        f("source_mac_address", "Source MAC", LIST, false, ""),
        f("source_hostname", "Source hostname", LIST, false, ""),
        f(
            "invert",
            "Invert match",
            Bool,
            false,
            "Match everything this rule would not.",
        ),
    ]);
    v
}

fn route_options_action() -> Vec<Field> {
    vec![
        f("override_address", "Override address", Text, false, ""),
        f("override_port", "Override port", Number, false, ""),
        f(
            "network_strategy",
            "Network strategy",
            Enum(&["default", "hybrid", "fallback"]),
            false,
            "",
        ),
        f("fallback_delay", "Fallback delay", Duration, false, ""),
        f(
            "udp_disable_domain_unmapping",
            "UDP: disable domain unmapping",
            Bool,
            false,
            "",
        ),
        f("udp_connect", "UDP connect", Bool, false, ""),
        f("udp_timeout", "UDP timeout", Duration, false, ""),
        f("tls_fragment", "TLS fragment", Bool, false, ""),
        f(
            "tls_fragment_fallback_delay",
            "TLS fragment fallback delay",
            Duration,
            false,
            "",
        ),
        f(
            "tls_record_fragment",
            "TLS record fragment",
            Bool,
            false,
            "",
        ),
    ]
}

pub fn fields(obj: Object, v: &Value) -> Vec<Field> {
    let t = v["type"].as_str().unwrap_or("");
    let mut out = vec![];
    match obj {
        Object::Outbound => match t {
            "selector" => out.extend([
                f(
                    "outbounds",
                    "Members",
                    Refs(Ns::Outbound),
                    true,
                    "Proxies you can choose between.",
                ),
                f(
                    "default",
                    "Default",
                    Ref(Ns::Outbound),
                    true,
                    "Used on start until you choose another.",
                ),
                f(
                    "interrupt_exist_connections",
                    "Interrupt connections on switch",
                    Bool,
                    false,
                    "",
                ),
            ]),
            "urltest" => out.extend([
                f(
                    "outbounds",
                    "Members",
                    Refs(Ns::Outbound),
                    true,
                    "The fastest member is used.",
                ),
                f(
                    "url",
                    "Test URL",
                    Text,
                    true,
                    "Default https://www.gstatic.com/generate_204",
                ),
                f("interval", "Test interval", Duration, true, "Default 3m"),
                f(
                    "tolerance",
                    "Tolerance (ms)",
                    Number,
                    true,
                    "Only switch when faster by this much. Default 50.",
                ),
                f(
                    "idle_timeout",
                    "Idle timeout",
                    Duration,
                    false,
                    "Default 30m",
                ),
                f(
                    "interrupt_exist_connections",
                    "Interrupt connections on switch",
                    Bool,
                    false,
                    "",
                ),
            ]),
            "direct" => out.extend(dial(false)),
            "block" => {}
            "shadowsocks" => {
                out.extend(server());
                out.extend([
                    f(
                        "method",
                        "Method",
                        Enum(&[
                            "2022-blake3-aes-128-gcm",
                            "2022-blake3-aes-256-gcm",
                            "2022-blake3-chacha20-poly1305",
                            "aes-128-gcm",
                            "aes-192-gcm",
                            "aes-256-gcm",
                            "chacha20-ietf-poly1305",
                            "xchacha20-ietf-poly1305",
                            "none",
                        ]),
                        true,
                        "",
                    ),
                    f("password", "Password", Secret, true, ""),
                    f(
                        "plugin",
                        "Plugin",
                        Enum(&["obfs-local", "v2ray-plugin"]),
                        false,
                        "",
                    ),
                    f("plugin_opts", "Plugin options", Text, false, ""),
                    f(
                        "network",
                        "Network",
                        Enum(&["tcp", "udp"]),
                        false,
                        "Both when empty.",
                    ),
                    f("udp_over_tcp", "UDP over TCP", Json, false, ""),
                    f("multiplex", "Multiplex", Json, false, ""),
                ]);
                out.extend(dial(false));
            }
            "vmess" | "vless" | "trojan" => {
                out.extend(server());
                if t == "trojan" {
                    out.push(f("password", "Password", Secret, true, ""));
                } else {
                    out.push(f("uuid", "UUID", Secret, true, ""));
                }
                if t == "vmess" {
                    out.extend([
                        f(
                            "security",
                            "Security",
                            Enum(&["auto", "none", "zero", "aes-128-gcm", "chacha20-poly1305"]),
                            false,
                            "",
                        ),
                        f("alter_id", "Alter ID", Number, false, ""),
                        f("global_padding", "Global padding", Bool, false, ""),
                        f(
                            "authenticated_length",
                            "Authenticated length",
                            Bool,
                            false,
                            "",
                        ),
                    ]);
                }
                if t == "vless" {
                    out.push(f("flow", "Flow", Enum(&["xtls-rprx-vision"]), true, ""));
                }
                if t != "trojan" {
                    out.push(f(
                        "packet_encoding",
                        "Packet encoding",
                        Enum(&["packetaddr", "xudp"]),
                        false,
                        "",
                    ));
                }
                out.push(f(
                    "network",
                    "Network",
                    Enum(&["tcp", "udp"]),
                    false,
                    "Both when empty.",
                ));
                out.extend(tls_transport(true));
                out.extend(dial(false));
            }
            "hysteria2" | "hysteria" => {
                out.extend(server());
                out.extend([
                    f(
                        "server_ports",
                        "Port hopping ranges",
                        LIST,
                        false,
                        "e.g. 2080:3000",
                    ),
                    f("hop_interval", "Hop interval", Duration, false, ""),
                    f("up_mbps", "Upload Mbps", Number, false, ""),
                    f("down_mbps", "Download Mbps", Number, false, ""),
                    f("password", "Password", Secret, true, ""),
                    f("obfs", "Obfuscation", Json, false, ""),
                    f("network", "Network", Enum(&["tcp", "udp"]), false, ""),
                    f("tls", "TLS", Json, true, ""),
                ]);
                out.extend(dial(false));
            }
            "tuic" => {
                out.extend(server());
                out.extend([
                    f("uuid", "UUID", Secret, true, ""),
                    f("password", "Password", Secret, true, ""),
                    f(
                        "congestion_control",
                        "Congestion control",
                        Enum(&["cubic", "new_reno", "bbr"]),
                        false,
                        "",
                    ),
                    f(
                        "udp_relay_mode",
                        "UDP relay mode",
                        Enum(&["native", "quic"]),
                        false,
                        "",
                    ),
                    f("udp_over_stream", "UDP over stream", Bool, false, ""),
                    f("zero_rtt_handshake", "0-RTT handshake", Bool, false, ""),
                    f("heartbeat", "Heartbeat", Duration, false, ""),
                    f("network", "Network", Enum(&["tcp", "udp"]), false, ""),
                    f("tls", "TLS", Json, true, ""),
                ]);
                out.extend(dial(false));
            }
            "anytls" => {
                out.extend(server());
                out.extend([
                    f("password", "Password", Secret, true, ""),
                    f(
                        "idle_session_check_interval",
                        "Idle session check",
                        Duration,
                        false,
                        "",
                    ),
                    f(
                        "idle_session_timeout",
                        "Idle session timeout",
                        Duration,
                        false,
                        "",
                    ),
                    f(
                        "min_idle_session",
                        "Minimum idle sessions",
                        Number,
                        false,
                        "",
                    ),
                    f("tls", "TLS", Json, true, ""),
                ]);
                out.extend(dial(false));
            }
            "socks" | "http" => {
                out.extend(server());
                if t == "socks" {
                    out.push(f(
                        "version",
                        "Version",
                        Enum(&["4", "4a", "5"]),
                        false,
                        "Default 5",
                    ));
                }
                out.extend([
                    f("username", "Username", Text, false, ""),
                    f("password", "Password", Secret, false, ""),
                ]);
                if t == "http" {
                    out.extend([
                        f("path", "Path", Text, false, ""),
                        f("headers", "Headers", Json, false, ""),
                        f("tls", "TLS", Json, false, ""),
                    ]);
                } else {
                    out.extend([
                        f("network", "Network", Enum(&["tcp", "udp"]), false, ""),
                        f("udp_over_tcp", "UDP over TCP", Json, false, ""),
                    ]);
                }
                out.extend(dial(false));
            }
            "shadowtls" => {
                out.extend(server());
                out.extend([
                    f("version", "Version", Number, true, "1, 2 or 3"),
                    f("password", "Password", Secret, true, ""),
                    f("tls", "TLS", Json, true, ""),
                ]);
                out.extend(dial(false));
            }
            "ssh" => {
                out.extend(server());
                out.extend([
                    f("user", "User", Text, true, ""),
                    f("password", "Password", Secret, false, ""),
                    f("private_key", "Private key", Secret, false, ""),
                    f("private_key_path", "Private key path", Text, false, ""),
                    f(
                        "private_key_passphrase",
                        "Key passphrase",
                        Secret,
                        false,
                        "",
                    ),
                    f("host_key", "Host keys", LIST, false, ""),
                    f(
                        "host_key_algorithms",
                        "Host key algorithms",
                        LIST,
                        false,
                        "",
                    ),
                    f("client_version", "Client version", Text, false, ""),
                ]);
                out.extend(dial(false));
            }
            "tor" => {
                out.extend([
                    f("executable_path", "Tor executable", Text, false, ""),
                    f("extra_args", "Extra arguments", LIST, false, ""),
                    f("data_directory", "Data directory", Text, false, ""),
                    f("torrc", "torrc options", Json, false, ""),
                ]);
                out.extend(dial(false));
            }
            _ => {
                out.extend(server());
                out.extend(dial(false));
            }
        },
        Object::Inbound => match t {
            "tun" => out.extend([
                f(
                    "interface_name",
                    "Interface name",
                    Text,
                    false,
                    "Chosen automatically when empty.",
                ),
                f(
                    "address",
                    "Addresses",
                    LIST,
                    true,
                    "e.g. 172.19.0.1/30, fdfe:dcba:9876::1/126",
                ),
                f("mtu", "MTU", Number, false, "Default 9000"),
                f(
                    "auto_route",
                    "Set routes automatically",
                    Bool,
                    true,
                    "Send system traffic into TUN.",
                ),
                f(
                    "strict_route",
                    "Strict route",
                    Bool,
                    true,
                    "Prevents leaks; can affect local network access.",
                ),
                f(
                    "stack",
                    "Stack",
                    Enum(&["system", "gvisor", "mixed"]),
                    true,
                    "mixed: system TCP + gVisor UDP.",
                ),
                f(
                    "route_address",
                    "Route only",
                    LIST,
                    false,
                    "Only these CIDRs go into TUN.",
                ),
                f(
                    "route_exclude_address",
                    "Exclude from route",
                    LIST,
                    false,
                    "CIDRs that bypass TUN.",
                ),
                f(
                    "route_address_set",
                    "Route rule sets",
                    Refs(Ns::RuleSet),
                    false,
                    "",
                ),
                f(
                    "route_exclude_address_set",
                    "Exclude rule sets",
                    Refs(Ns::RuleSet),
                    false,
                    "",
                ),
                f("dns_mode", "DNS mode", Text, false, ""),
                f("dns_address", "DNS addresses", LIST, false, ""),
                f("auto_redirect", "Auto redirect", Bool, false, "Linux only."),
                f(
                    "endpoint_independent_nat",
                    "Endpoint-independent NAT",
                    Bool,
                    false,
                    "",
                ),
                f(
                    "include_interface",
                    "Include interfaces",
                    LIST,
                    false,
                    "Linux only.",
                ),
                f(
                    "exclude_interface",
                    "Exclude interfaces",
                    LIST,
                    false,
                    "Linux only.",
                ),
                f("include_uid", "Include UIDs", NUMBERS, false, "Linux only."),
                f("exclude_uid", "Exclude UIDs", NUMBERS, false, "Linux only."),
                f(
                    "include_package",
                    "Include packages",
                    LIST,
                    false,
                    "Android only.",
                ),
                f(
                    "exclude_package",
                    "Exclude packages",
                    LIST,
                    false,
                    "Android only.",
                ),
                f("iproute2_table_index", "iproute2 table", Number, false, ""),
                f(
                    "iproute2_rule_index",
                    "iproute2 rule index",
                    Number,
                    false,
                    "",
                ),
                f("loopback_address", "Loopback addresses", LIST, false, ""),
                f("platform", "Platform options", Json, false, ""),
                f("udp_timeout", "UDP timeout", Duration, false, ""),
            ]),
            "mixed" | "socks" | "http" => {
                out.extend(listen());
                out.push(f("users", "Users", Json, false, "[{\"username\":\"…\",\"password\":\"…\"}] — empty allows everyone who can connect."));
                if t != "socks" {
                    out.push(f(
                        "set_system_proxy",
                        "Set system proxy",
                        Bool,
                        false,
                        "sing manages the macOS system proxy itself; leave off.",
                    ));
                }
                if t == "http" {
                    out.push(f("tls", "TLS", Json, false, ""));
                }
            }
            "direct" => {
                out.extend(listen());
                out.extend([
                    f("network", "Network", Enum(&["tcp", "udp"]), false, ""),
                    f("override_address", "Override address", Text, false, ""),
                    f("override_port", "Override port", Number, false, ""),
                ]);
            }
            _ => {
                out.extend(listen());
                out.push(f("users", "Users", Json, false, ""));
            }
        },
        Object::Endpoint => match t {
            "wireguard" => {
                out.extend([
                    f("address", "Addresses", LIST, true, ""),
                    f("private_key", "Private key", Secret, true, ""),
                    f(
                        "peers",
                        "Peers",
                        Json,
                        true,
                        "[{\"address\",\"port\",\"public_key\",\"allowed_ips\"…}]",
                    ),
                    f("mtu", "MTU", Number, false, ""),
                    f("listen_port", "Listen port", Number, false, ""),
                    f("system", "System interface", Bool, false, ""),
                    f("name", "Interface name", Text, false, ""),
                    f("udp_timeout", "UDP timeout", Duration, false, ""),
                    f("workers", "Workers", Number, false, ""),
                ]);
                out.extend(dial(false));
            }
            "tailscale" => {
                out.extend([
                    f("auth_key", "Auth key", Secret, true, ""),
                    f("hostname", "Hostname", Text, true, ""),
                    f("state_directory", "State directory", Text, false, ""),
                    f("control_url", "Control URL", Text, false, ""),
                    f("ephemeral", "Ephemeral", Bool, false, ""),
                    f("accept_routes", "Accept routes", Bool, false, ""),
                    f("exit_node", "Exit node", Text, false, ""),
                    f(
                        "exit_node_allow_lan_access",
                        "Exit node LAN access",
                        Bool,
                        false,
                        "",
                    ),
                    f("advertise_routes", "Advertise routes", LIST, false, ""),
                    f(
                        "advertise_exit_node",
                        "Advertise exit node",
                        Bool,
                        false,
                        "",
                    ),
                    f("udp_timeout", "UDP timeout", Duration, false, ""),
                ]);
                out.extend(dial(false));
            }
            _ => {}
        },
        Object::DnsServer => match t {
            "local" => out.extend(dial(false)),
            "hosts" => out.extend([
                f("path", "Hosts files", LIST, true, "Default /etc/hosts"),
                f(
                    "predefined",
                    "Predefined entries",
                    Json,
                    true,
                    "{\"example.com\": [\"1.2.3.4\"]}",
                ),
            ]),
            "udp" | "tcp" | "tls" | "quic" | "https" | "h3" => {
                out.extend([
                    f(
                        "server",
                        "Server",
                        Text,
                        true,
                        "IP address, or a hostname resolved with the field below.",
                    ),
                    f(
                        "server_port",
                        "Port",
                        Number,
                        false,
                        "Protocol default when empty.",
                    ),
                ]);
                if t == "https" || t == "h3" {
                    out.extend([
                        f("path", "Path", Text, false, "Default /dns-query"),
                        f("headers", "Headers", Json, false, ""),
                    ]);
                }
                if ["tls", "quic", "https", "h3"].contains(&t) {
                    out.push(f("tls", "TLS", Json, false, ""));
                }
                out.extend(dial(true));
            }
            "dhcp" => {
                out.push(f(
                    "interface",
                    "Interface",
                    Text,
                    false,
                    "Default interface when empty.",
                ));
                out.extend(dial(false));
            }
            "fakeip" => out.extend([
                f(
                    "inet4_range",
                    "IPv4 range",
                    Text,
                    true,
                    "e.g. 198.18.0.0/15",
                ),
                f("inet6_range", "IPv6 range", Text, true, "e.g. fc00::/18"),
            ]),
            "tailscale" => out.extend([
                f("endpoint", "Tailscale endpoint", Text, true, ""),
                f(
                    "accept_default_resolvers",
                    "Accept default resolvers",
                    Bool,
                    false,
                    "",
                ),
            ]),
            "resolved" => out.extend([
                f("service", "Resolved service", Text, true, ""),
                f(
                    "accept_default_resolvers",
                    "Accept default resolvers",
                    Bool,
                    false,
                    "",
                ),
            ]),
            _ => {}
        },
        Object::DnsOptions => out.extend([
            f(
                "final",
                "Default server",
                Ref(Ns::Dns),
                true,
                "Used when no DNS rule matches. First server when empty.",
            ),
            f(
                "strategy",
                "Address preference",
                Enum(&["prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only"]),
                true,
                "",
            ),
            f(
                "independent_cache",
                "Separate cache per server",
                Bool,
                true,
                "",
            ),
            f("disable_cache", "Disable cache", Bool, false, ""),
            f("disable_expire", "Never expire cache", Bool, false, ""),
            f("cache_capacity", "Cache capacity", Number, false, ""),
            f(
                "optimistic",
                "Optimistic cache",
                Json,
                false,
                "true, or an object",
            ),
            f("timeout", "Timeout", Duration, false, ""),
            f(
                "reverse_mapping",
                "Reverse mapping",
                Bool,
                false,
                "Remember IP → domain for routing.",
            ),
            f("client_subnet", "Client subnet (ECS)", Text, false, ""),
            f("fakeip", "FakeIP (legacy)", Json, false, ""),
        ]),
        Object::DnsRule | Object::RouteRule | Object::HeadlessRule => {
            let dns = obj == Object::DnsRule;
            let headless = obj == Object::HeadlessRule;
            if t == "logical" {
                out.extend([
                    f(
                        "mode",
                        "Combine",
                        Enum(&["and", "or"]),
                        true,
                        "and: all must match · or: any",
                    ),
                    f(
                        "rules",
                        "Conditions",
                        NestedList(if dns {
                            Object::DnsRule
                        } else {
                            Object::HeadlessRule
                        }),
                        true,
                        "",
                    ),
                    f("invert", "Invert match", Bool, false, ""),
                ]);
            } else {
                out.extend(matchers(dns, headless));
            }
            if headless {
                return out;
            }
            let action = v["action"].as_str().unwrap_or("route");
            if dns {
                out.push(f(
                    "action",
                    "Action",
                    Enum(&[
                        "route",
                        "evaluate",
                        "respond",
                        "route-options",
                        "reject",
                        "predefined",
                    ]),
                    true,
                    "route: answer with a server · reject: block the query",
                ));
                match action {
                    "route" | "evaluate" => out.extend([
                        f("server", "Server", Ref(Ns::Dns), true, ""),
                        f(
                            "strategy",
                            "Address preference",
                            Enum(&["prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only"]),
                            false,
                            "",
                        ),
                        f("disable_cache", "Disable cache", Bool, false, ""),
                        f("rewrite_ttl", "Rewrite TTL", Number, false, ""),
                        f("client_subnet", "Client subnet", Text, false, ""),
                        f("timeout", "Timeout", Duration, false, ""),
                    ]),
                    "route-options" => out.extend([
                        f("disable_cache", "Disable cache", Bool, false, ""),
                        f("rewrite_ttl", "Rewrite TTL", Number, false, ""),
                        f("client_subnet", "Client subnet", Text, false, ""),
                        f("timeout", "Timeout", Duration, false, ""),
                    ]),
                    "reject" => out.extend([
                        f("method", "Method", Enum(&["default", "drop"]), false, ""),
                        f("no_drop", "Never drop", Bool, false, ""),
                    ]),
                    "predefined" => out.extend([
                        f(
                            "rcode",
                            "Response code",
                            Enum(&[
                                "NOERROR", "FORMERR", "SERVFAIL", "NXDOMAIN", "NOTIMP", "REFUSED",
                            ]),
                            true,
                            "",
                        ),
                        f(
                            "answer",
                            "Answer records",
                            LIST,
                            true,
                            "e.g. example.com. IN A 127.0.0.1",
                        ),
                        f("ns", "NS records", LIST, false, ""),
                        f("extra", "Extra records", LIST, false, ""),
                    ]),
                    _ => {}
                }
            } else {
                out.push(f(
                    "action",
                    "Action",
                    Enum(&[
                        "route",
                        "reject",
                        "hijack-dns",
                        "sniff",
                        "resolve",
                        "route-options",
                        "bypass",
                    ]),
                    true,
                    "route: send to a target · reject: block · sniff: detect domain and protocol",
                ));
                match action {
                    "route" | "bypass" => {
                        out.push(f("outbound", "Send to", Ref(Ns::Outbound), true, ""));
                        out.extend(route_options_action());
                    }
                    "route-options" => out.extend(route_options_action()),
                    "reject" => out.extend([
                        f(
                            "method",
                            "Method",
                            Enum(&["default", "drop", "reply"]),
                            false,
                            "",
                        ),
                        f("no_drop", "Never drop", Bool, false, ""),
                    ]),
                    "sniff" => out.extend([
                        f("sniffer", "Sniffers", LIST, false, "All when empty."),
                        f("timeout", "Timeout", Duration, false, "Default 300ms"),
                    ]),
                    "resolve" => out.extend([
                        f("server", "DNS server", Ref(Ns::Dns), false, ""),
                        f(
                            "strategy",
                            "Address preference",
                            Enum(&["prefer_ipv4", "prefer_ipv6", "ipv4_only", "ipv6_only"]),
                            false,
                            "",
                        ),
                        f("disable_cache", "Disable cache", Bool, false, ""),
                        f("rewrite_ttl", "Rewrite TTL", Number, false, ""),
                        f("client_subnet", "Client subnet", Text, false, ""),
                        f("timeout", "Timeout", Duration, false, ""),
                    ]),
                    _ => {}
                }
            }
        }
        Object::RouteOptions => out.extend([
            f(
                "final",
                "Unmatched traffic",
                Ref(Ns::Outbound),
                true,
                "Target when no rule matches.",
            ),
            f(
                "auto_detect_interface",
                "Detect outbound interface",
                Bool,
                true,
                "Recommended with TUN to avoid loops.",
            ),
            f(
                "find_process",
                "Identify apps",
                Bool,
                true,
                "Look up the app behind each connection. Needed for app names in Activity.",
            ),
            f(
                "default_domain_resolver",
                "Resolve server names with",
                Ref(Ns::Dns),
                true,
                "Default DNS server for outbound server hostnames.",
            ),
            f("default_interface", "Default interface", Text, false, ""),
            f(
                "default_mark",
                "Default routing mark",
                Number,
                false,
                "Linux only.",
            ),
            f(
                "override_android_vpn",
                "Override Android VPN",
                Bool,
                false,
                "",
            ),
            f(
                "default_network_strategy",
                "Network strategy",
                Enum(&["default", "hybrid", "fallback"]),
                false,
                "",
            ),
            f("default_network_type", "Network types", LIST, false, ""),
            f(
                "default_fallback_network_type",
                "Fallback network types",
                LIST,
                false,
                "",
            ),
            f(
                "default_fallback_delay",
                "Fallback delay",
                Duration,
                false,
                "",
            ),
            f("find_neighbor", "Identify LAN devices", Bool, false, ""),
            f("dhcp_lease_files", "DHCP lease files", LIST, false, ""),
            f(
                "default_http_client",
                "Default HTTP client",
                Text,
                false,
                "",
            ),
        ]),
        Object::RuleSet => match t {
            "inline" => out.push(f(
                "rules",
                "Rules",
                NestedList(Object::HeadlessRule),
                true,
                "",
            )),
            "local" => out.extend([
                f(
                    "format",
                    "Format",
                    Enum(&["source", "binary"]),
                    true,
                    "source: JSON · binary: .srs",
                ),
                f("path", "File", Text, true, ""),
            ]),
            "remote" => out.extend([
                f(
                    "format",
                    "Format",
                    Enum(&["source", "binary"]),
                    true,
                    "source: JSON · binary: .srs",
                ),
                f("url", "URL", Text, true, ""),
                f(
                    "update_interval",
                    "Update every",
                    Duration,
                    true,
                    "Default 1d",
                ),
                f(
                    "download_detour",
                    "Download via",
                    Ref(Ns::Outbound),
                    false,
                    "Default: direct",
                ),
                f("http_client", "HTTP client", Text, false, ""),
            ]),
            _ => {}
        },
        Object::Log => out.extend([
            f(
                "level",
                "Level",
                Enum(&["trace", "debug", "info", "warn", "error", "fatal", "panic"]),
                true,
                "",
            ),
            f("timestamp", "Timestamps", Bool, true, ""),
            f(
                "output",
                "Output file",
                Text,
                false,
                "sing reads core.log; changing this hides logs from Activity.",
            ),
            f("disabled", "Disable logging", Bool, false, ""),
        ]),
        Object::Ntp => {
            out.extend([
                f(
                    "enabled",
                    "Enabled",
                    Bool,
                    true,
                    "Use NTP time for TLS where the system clock is wrong.",
                ),
                f("server", "Server", Text, true, "e.g. time.apple.com"),
                f("server_port", "Port", Number, false, "Default 123"),
                f("interval", "Interval", Duration, false, "Default 30m"),
            ]);
            out.extend(dial(false));
        }
        Object::Certificate => out.extend([
            f(
                "store",
                "Trusted store",
                Enum(&["system", "mozilla", "chrome", "none"]),
                true,
                "",
            ),
            f("certificate", "Extra certificates (PEM)", LIST, false, ""),
            f("certificate_path", "Certificate files", LIST, false, ""),
            f(
                "certificate_directory_path",
                "Certificate directories",
                LIST,
                false,
                "",
            ),
        ]),
        Object::Experimental => out.extend([
            f(
                "cache_file",
                "Cache file",
                Nested(Object::CacheFile),
                true,
                "Remember selections, FakeIP and DNS across restarts.",
            ),
            f(
                "clash_api",
                "Clash API",
                Nested(Object::ClashApi),
                false,
                "sing uses it for live mode switching.",
            ),
            f("v2ray_api", "V2Ray API", Json, false, ""),
        ]),
        Object::CacheFile => out.extend([
            f(
                "enabled",
                "Enabled",
                Bool,
                true,
                "When on, the core restores group selections itself.",
            ),
            f("path", "Path", Text, false, "Default cache.db"),
            f("cache_id", "Cache ID", Text, false, ""),
            f("store_fakeip", "Store FakeIP", Bool, false, ""),
            f("store_dns", "Store DNS cache", Bool, false, ""),
            f(
                "store_rdrc",
                "Store rejected DNS (deprecated)",
                Bool,
                false,
                "",
            ),
            f("rdrc_timeout", "Rejected DNS timeout", Duration, false, ""),
        ]),
        Object::ClashApi => out.extend([
            f(
                "external_controller",
                "Controller address",
                Text,
                true,
                "Leave empty unless you use a Clash dashboard; it opens an HTTP port.",
            ),
            f("secret", "Secret", Secret, false, ""),
            f("external_ui", "Dashboard directory", Text, false, ""),
            f(
                "external_ui_download_url",
                "Dashboard download URL",
                Text,
                false,
                "",
            ),
            f(
                "external_ui_download_detour",
                "Dashboard download via",
                Ref(Ns::Outbound),
                false,
                "",
            ),
            f(
                "default_mode",
                "Default mode",
                Text,
                false,
                "sing sets this from the Mode switch.",
            ),
            f(
                "access_control_allow_origin",
                "Allowed origins",
                LIST,
                false,
                "",
            ),
            f(
                "access_control_allow_private_network",
                "Allow private network",
                Bool,
                false,
                "",
            ),
        ]),
        Object::Service => {
            if t == "api" {
                out.extend([
                    f(
                        "listen",
                        "Listen",
                        Text,
                        true,
                        "sing's management API. Keep 127.0.0.1.",
                    ),
                    f("listen_port", "Port", Number, true, ""),
                ]);
            } else {
                out.extend(listen());
            }
        }
        Object::Unknown => {}
    }
    out
}

/// Starting points offered by "New". Tags are made unique by the caller.
pub fn templates(obj: Object) -> Vec<(&'static str, &'static str, Value)> {
    match obj {
        Object::DnsServer => vec![
            (
                "Cloudflare",
                "DNS over HTTPS · 1.1.1.1",
                json!({"type":"https","tag":"cloudflare","server":"1.1.1.1"}),
            ),
            (
                "Google",
                "DNS over HTTPS · 8.8.8.8",
                json!({"type":"https","tag":"google","server":"8.8.8.8"}),
            ),
            (
                "Quad9",
                "DNS over HTTPS · 9.9.9.9",
                json!({"type":"https","tag":"quad9","server":"9.9.9.9"}),
            ),
            (
                "DNSPod",
                "DNS over HTTPS · 1.12.12.12",
                json!({"type":"https","tag":"dnspod","server":"1.12.12.12"}),
            ),
            (
                "System",
                "Resolver of this computer",
                json!({"type":"local","tag":"system"}),
            ),
            (
                "DHCP",
                "Resolver announced by the network",
                json!({"type":"dhcp","tag":"dhcp"}),
            ),
            (
                "FakeIP",
                "Fake addresses for TUN setups",
                json!({"type":"fakeip","tag":"fakeip","inet4_range":"198.18.0.0/15","inet6_range":"fc00::/18"}),
            ),
            (
                "Hosts",
                "Static answers from hosts files",
                json!({"type":"hosts","tag":"hosts"}),
            ),
            (
                "Custom UDP",
                "Plain DNS",
                json!({"type":"udp","tag":"dns-udp","server":""}),
            ),
            (
                "Custom TLS",
                "DNS over TLS",
                json!({"type":"tls","tag":"dns-tls","server":""}),
            ),
            (
                "Custom HTTPS",
                "DNS over HTTPS",
                json!({"type":"https","tag":"dns-https","server":""}),
            ),
            (
                "Custom QUIC",
                "DNS over QUIC",
                json!({"type":"quic","tag":"dns-quic","server":""}),
            ),
            (
                "Custom HTTP/3",
                "DNS over HTTP/3",
                json!({"type":"h3","tag":"dns-h3","server":""}),
            ),
        ],
        Object::DnsRule => vec![
            (
                "Domains → server",
                "Resolve matching domains with a server",
                json!({"domain_suffix":[],"action":"route","server":""}),
            ),
            (
                "Rule set → server",
                "Resolve a rule set's domains with a server",
                json!({"rule_set":[],"action":"route","server":""}),
            ),
            (
                "Block domains",
                "Reject queries, e.g. ads",
                json!({"rule_set":[],"action":"reject"}),
            ),
            (
                "Combined condition",
                "All / any of several conditions",
                json!({"type":"logical","mode":"and","rules":[],"action":"route","server":""}),
            ),
        ],
        Object::RouteRule | Object::HeadlessRule => {
            let mut v = vec![
                (
                    "Domain",
                    "Match domains",
                    json!({"domain_suffix":[],"action":"route","outbound":""}),
                ),
                (
                    "App",
                    "Match an application (process)",
                    json!({"process_name":[],"action":"route","outbound":""}),
                ),
                (
                    "IP range",
                    "Match destination addresses",
                    json!({"ip_cidr":[],"action":"route","outbound":""}),
                ),
                (
                    "Rule set",
                    "Match a rule set",
                    json!({"rule_set":[],"action":"route","outbound":""}),
                ),
                (
                    "Combined condition",
                    "App and domain, or several domains…",
                    json!({"type":"logical","mode":"and","rules":[],"action":"route","outbound":""}),
                ),
                (
                    "Sniff",
                    "Detect domain and protocol first",
                    json!({"action":"sniff"}),
                ),
                (
                    "Hijack DNS",
                    "Answer DNS queries with sing-box DNS",
                    json!({"protocol":["dns"],"action":"hijack-dns"}),
                ),
                (
                    "Private IPs direct",
                    "Keep LAN traffic local",
                    json!({"ip_is_private":true,"action":"route","outbound":"direct"}),
                ),
            ];
            if obj == Object::HeadlessRule {
                v.truncate(5);
                v.remove(3);
                for (_, _, value) in &mut v {
                    let m = value.as_object_mut().unwrap();
                    m.remove("action");
                    m.remove("outbound");
                }
            }
            v
        }
        Object::RuleSet => vec![
            (
                "Remote",
                "Downloaded and updated by the core (.srs or JSON)",
                json!({"type":"remote","tag":"rules","format":"binary","url":""}),
            ),
            (
                "Local file",
                "A rule-set file on this computer",
                json!({"type":"local","tag":"rules","format":"source","path":""}),
            ),
            (
                "Inline",
                "Rules written here",
                json!({"type":"inline","tag":"rules","rules":[]}),
            ),
        ],
        Object::Inbound => vec![
            (
                "Proxy port",
                "HTTP + SOCKS on one port",
                json!({"type":"mixed","tag":"mixed","listen":"127.0.0.1","listen_port":2081}),
            ),
            (
                "SOCKS",
                "",
                json!({"type":"socks","tag":"socks","listen":"127.0.0.1","listen_port":1080}),
            ),
            (
                "HTTP",
                "",
                json!({"type":"http","tag":"http","listen":"127.0.0.1","listen_port":8080}),
            ),
            (
                "TUN",
                "Capture all traffic (administrator)",
                crate::native::tun_template(),
            ),
            (
                "Other",
                "Any inbound type",
                json!({"type":"direct","tag":"inbound"}),
            ),
        ],
        Object::Outbound => vec![
            (
                "Shadowsocks",
                "",
                json!({"type":"shadowsocks","tag":"shadowsocks","server":"","server_port":8388,"method":"2022-blake3-aes-128-gcm","password":""}),
            ),
            (
                "VMess",
                "",
                json!({"type":"vmess","tag":"vmess","server":"","server_port":443,"uuid":""}),
            ),
            (
                "VLESS",
                "",
                json!({"type":"vless","tag":"vless","server":"","server_port":443,"uuid":""}),
            ),
            (
                "Trojan",
                "",
                json!({"type":"trojan","tag":"trojan","server":"","server_port":443,"password":"","tls":{"enabled":true}}),
            ),
            (
                "Hysteria2",
                "",
                json!({"type":"hysteria2","tag":"hysteria2","server":"","server_port":443,"password":"","tls":{"enabled":true}}),
            ),
            (
                "TUIC",
                "",
                json!({"type":"tuic","tag":"tuic","server":"","server_port":443,"uuid":"","tls":{"enabled":true}}),
            ),
            (
                "AnyTLS",
                "",
                json!({"type":"anytls","tag":"anytls","server":"","server_port":443,"password":"","tls":{"enabled":true}}),
            ),
            (
                "SOCKS",
                "",
                json!({"type":"socks","tag":"socks","server":"127.0.0.1","server_port":1080}),
            ),
            (
                "HTTP",
                "",
                json!({"type":"http","tag":"http","server":"127.0.0.1","server_port":8080}),
            ),
            (
                "SSH",
                "",
                json!({"type":"ssh","tag":"ssh","server":"","server_port":22,"user":""}),
            ),
            (
                "Direct",
                "Another direct outbound, e.g. bound to an interface",
                json!({"type":"direct","tag":"direct-2"}),
            ),
        ],
        Object::Endpoint => vec![
            (
                "WireGuard",
                "",
                json!({"type":"wireguard","tag":"wireguard","address":[],"private_key":"","peers":[]}),
            ),
            (
                "Tailscale",
                "",
                json!({"type":"tailscale","tag":"tailscale"}),
            ),
        ],
        Object::Service => vec![
            (
                "Resolved",
                "systemd-resolved compatible DNS service",
                json!({"type":"resolved","tag":"resolved","listen":"127.0.0.53","listen_port":53}),
            ),
            (
                "Other",
                "Any service type",
                json!({"type":"derp","tag":"service"}),
            ),
        ],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointers_map_to_objects_and_actions_change_fields() {
        assert_eq!(Object::for_pointer("/dns/servers/3"), Object::DnsServer);
        assert_eq!(Object::for_pointer("/route/rules/-"), Object::RouteRule);
        assert_eq!(Object::for_pointer("/route"), Object::RouteOptions);
        let route = fields(Object::RouteRule, &json!({"action":"route"}));
        assert!(route.iter().any(|f| f.key == "outbound"));
        let reject = fields(Object::RouteRule, &json!({"action":"reject"}));
        assert!(!reject.iter().any(|f| f.key == "outbound"));
        let headless = fields(Object::HeadlessRule, &json!({}));
        assert!(!headless
            .iter()
            .any(|f| f.key == "action" || f.key == "rule_set"));
        for obj in [
            Object::DnsServer,
            Object::RouteRule,
            Object::Outbound,
            Object::Inbound,
        ] {
            for (_, _, v) in templates(obj) {
                let keys: Vec<_> = fields(obj, &v).iter().map(|f| f.key).collect();
                let mut unique = keys.clone();
                unique.sort();
                unique.dedup();
                assert_eq!(keys.len(), unique.len(), "{obj:?} {v}");
            }
        }
    }
}
