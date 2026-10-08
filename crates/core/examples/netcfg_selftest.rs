// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/netcfg_selftest.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Verifies IP editing against NetworkManager using a throw-away,
//           inactive profile (never touches real connections).
//           cargo run -p syspulse-core --example netcfg_selftest
// =============================================================================

use std::process::Command;

use syspulse_core::netconfig;

fn main() {
    let add = Command::new("nmcli")
        .args([
            "-t",
            "connection",
            "add",
            "type",
            "ethernet",
            "ifname",
            "spself0",
            "con-name",
            "syspulse-selftest",
            "autoconnect",
            "no",
        ])
        .output()
        .expect("nmcli");
    assert!(
        add.status.success(),
        "create test profile: {}",
        String::from_utf8_lossy(&add.stderr)
    );
    let uuid = netconfig::connections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "syspulse-selftest")
        .unwrap()
        .uuid;

    let result = std::panic::catch_unwind(|| {
        let mut s = netconfig::connection_settings(&uuid).unwrap();
        println!("initial: ipv4 {} {:?}", s.ipv4.method, s.ipv4.addresses);
        s.ipv4.method = "manual".into();
        s.ipv4.addresses = vec!["10.99.0.10/24".into(), "192.168.250.10/24".into()];
        s.ipv4.gateway = Some("10.99.0.1".into());
        s.ipv4.dns = vec!["1.1.1.1".into(), "8.8.8.8".into()];
        s.mtu = Some(1400);
        s.autoconnect = false;
        println!("{}", netconfig::update_connection(&s, false).unwrap());
        let r = netconfig::connection_settings(&uuid).unwrap();
        println!(
            "static: {} {:?} gw {:?} dns {:?} mtu {:?}",
            r.ipv4.method, r.ipv4.addresses, r.ipv4.gateway, r.ipv4.dns, r.mtu
        );
        assert_eq!(r.ipv4.method, "manual");
        assert_eq!(r.ipv4.addresses.len(), 2);
        assert_eq!(r.ipv4.gateway.as_deref(), Some("10.99.0.1"));
        assert_eq!(r.mtu, Some(1400));

        let mut back = r.clone();
        back.ipv4.method = "auto".into();
        back.ipv4.addresses.clear();
        back.ipv4.gateway = None;
        back.ipv4.dns.clear();
        back.mtu = None;
        netconfig::update_connection(&back, false).unwrap();
        let r = netconfig::connection_settings(&uuid).unwrap();
        println!(
            "dhcp again: {} {:?} gw {:?} mtu {:?}",
            r.ipv4.method, r.ipv4.addresses, r.ipv4.gateway, r.mtu
        );
        assert_eq!(r.ipv4.method, "auto");
        assert!(r.ipv4.addresses.is_empty());

        let mut bad = r.clone();
        bad.ipv4.method = "manual".into();
        bad.ipv4.addresses = vec!["10.0.0.300/24".into()];
        assert!(
            netconfig::update_connection(&bad, false).is_err(),
            "invalid IP must be rejected before nmcli"
        );
        println!("validation ok");
    });
    let del = Command::new("nmcli")
        .args(["connection", "delete", &uuid])
        .output()
        .unwrap();
    println!(
        "cleanup: {}",
        if del.status.success() {
            "test profile deleted"
        } else {
            "DELETE FAILED"
        }
    );
    result.unwrap();
}
