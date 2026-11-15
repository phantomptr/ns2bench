use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::Ipv4Addr;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../tests/fixtures/reference.json")).unwrap()
}
fn bytes(s: &str) -> Vec<u8> {
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn config() -> Config {
    Config::from_value(&fixture()["config"]).unwrap()
}

#[test]
fn dhcp_matches_reference_packets_and_leases() {
    let cfg = config();
    for case in fixture()["dhcp"].as_array().unwrap() {
        let req = Dhcp::parse(&bytes(case["packet"].as_str().unwrap())).unwrap();
        let mut leases = BTreeMap::new();
        let result = handle_dhcp(&cfg, &mut leases, &req, 1000.0, "test-time")
            .map(|(kind, ip)| cfg.dhcp_reply(&req, kind, ip));
        assert_eq!(result, case["reply"].as_str().map(bytes));
        let mut expected = case["leases"].clone();
        for lease in expected.as_object_mut().unwrap().values_mut() {
            lease["expires"] = json!(lease["expires"].as_f64().unwrap());
        }
        assert_eq!(serde_json::to_value(leases).unwrap(), expected);
    }
}

#[cfg(unix)]
#[test]
fn socket_is_pinned_and_missing_interfaces_fail_closed() {
    use std::os::fd::AsRawFd;
    let interface = if cfg!(target_os = "macos") {
        "lo0"
    } else {
        "lo"
    };
    let fd = runtime::bound_socket(libc::SOCK_DGRAM, interface, Ipv4Addr::LOCALHOST, 0).unwrap();
    assert!(runtime::socket_is_bound(fd.as_raw_fd(), interface));
    assert!(
        runtime::bound_socket(libc::SOCK_DGRAM, "ns2-no-such-if", Ipv4Addr::LOCALHOST, 0).is_err()
    );
}

#[cfg(unix)]
#[test]
fn lease_file_survives_restart_and_corruption_is_not_silently_reset() {
    let path =
        std::env::temp_dir().join(format!("ns2bench-netd-leases-{}.json", std::process::id()));
    let mut leases = BTreeMap::new();
    leases.insert(
        "aa:bb:cc:dd:ee:ff".into(),
        json!({"ip":"192.168.80.10","expires":1234}),
    );
    runtime::save_leases(&path, &leases).unwrap();
    assert_eq!(runtime::load_leases(&path).unwrap(), leases);
    std::fs::write(&path, b"not json").unwrap();
    assert!(runtime::load_leases(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[cfg(unix)]
#[test]
fn forwarding_guard_refuses_enabled_missing_or_invalid_values() {
    assert!(runtime::forwarding_values(Some("0"), Some("0")));
    for (v4, v6) in [
        (Some("1"), Some("0")),
        (Some("0"), Some("1")),
        (None, Some("0")),
        (Some("0"), None),
        (Some("oops"), Some("0")),
    ] {
        assert!(!runtime::forwarding_values(v4, v6));
    }
}

#[test]
fn dns_matches_reference_and_never_forwards() {
    let cfg = config();
    for case in fixture()["dns"].as_array().unwrap() {
        assert_eq!(
            cfg.dns_reply(&bytes(case["packet"].as_str().unwrap()))
                .unwrap()
                .0,
            bytes(case["reply"].as_str().unwrap())
        );
    }
}

#[test]
fn malformed_and_truncated_packets_are_ignored() {
    let cfg = config();
    let packet = bytes(fixture()["dhcp"][0]["packet"].as_str().unwrap());
    for end in 0..packet.len() - 1 {
        assert!(Dhcp::parse(&packet[..end]).is_none(), "length {end}");
    }
    let mut relay = packet.clone();
    relay[24] = 10;
    assert!(Dhcp::parse(&relay).is_none());
    let mut wrong_hardware = packet;
    wrong_hardware[2] = 16;
    assert!(Dhcp::parse(&wrong_hardware).is_none());
    let packet = bytes(fixture()["dns"][0]["packet"].as_str().unwrap());
    for end in 0..packet.len() {
        assert!(cfg.dns_reply(&packet[..end]).is_none());
    }
    let mut compressed = packet.clone();
    compressed[12] = 0xc0;
    assert!(cfg.dns_reply(&compressed).is_none());
    let mut response = packet;
    response[2] |= 0x80;
    assert!(cfg.dns_reply(&response).is_none());
}

#[test]
fn allocation_respects_reservations_and_active_leases() {
    let mut data = fixture()["config"].clone();
    data["network"]["reservations"] = json!({"11:22:33:44:55:66":"192.168.80.10"});
    let cfg = Config::from_value(&data).unwrap();
    let mut leases = BTreeMap::new();
    assert_eq!(
        choose(&cfg, &leases, "aa:bb:cc:dd:ee:ff", 1000.0),
        Some(Ipv4Addr::new(192, 168, 80, 11))
    );
    leases.insert(
        "00:00:00:00:00:01".into(),
        json!({"ip":"192.168.80.11","expires":2000}),
    );
    assert_eq!(choose(&cfg, &leases, "aa:bb:cc:dd:ee:ff", 1000.0), None);
    assert_eq!(
        choose(&cfg, &leases, "aa:bb:cc:dd:ee:ff", 3000.0),
        Some(Ipv4Addr::new(192, 168, 80, 11))
    );
    // A stale assignment must not override a new reservation for somebody else.
    leases.insert(
        "aa:bb:cc:dd:ee:ff".into(),
        json!({"ip":"192.168.80.10","expires":4000}),
    );
    assert_eq!(
        choose(&cfg, &leases, "aa:bb:cc:dd:ee:ff", 3000.0),
        Some(Ipv4Addr::new(192, 168, 80, 11))
    );
}

#[test]
fn config_rejects_non_private_or_invalid_addresses() {
    for (key, value) in [
        ("address", "8.8.8.8"),
        ("pool_start", "192.168.80.1"),
        ("pool_end", "192.168.81.1"),
        ("pool_end", "192.168.80.255"),
        ("subnet", "0.0.0.0/0"),
        ("pool_start", "192.168.80.12"),
    ] {
        let mut data = fixture()["config"].clone();
        data["network"][key] = value.into();
        assert!(Config::from_value(&data).is_err(), "{key}={value}");
    }
}

#[test]
fn only_local_connectivity_http_requests_succeed() {
    assert!(
        http_reply(b"GET / HTTP/1.1\r\nHost: ctest.p01.ctest.srv.nintendo.net\r\n\r\n")
            .starts_with(b"HTTP/1.1 200")
    );
    for req in [
        b"GET / HTTP/1.1\r\nHost: example.invalid\r\n\r\n".as_slice(),
        b"POST / HTTP/1.1\r\nHost: ctest.cdn.nintendo.net\r\n\r\n",
        b"GET /other HTTP/1.1\r\nHost: ctest.cdn.nintendo.net\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: ctest.cdn.nintendo.net\r\nHost: example.invalid\r\n\r\n",
    ] {
        assert!(http_reply(req).starts_with(b"HTTP/1.1 404"));
    }
}

#[cfg(unix)]
#[test]
fn service_bind_address_preserves_macos_unprivileged_operation() {
    let ip = "192.168.80.1".parse().unwrap();
    assert_eq!(
        runtime::service_address(ip),
        if cfg!(target_os = "macos") {
            std::net::Ipv4Addr::UNSPECIFIED
        } else {
            ip
        }
    );
}
