// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/netperf.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Command-line speed test (handy for headless machines).
//             cargo run --release -p syspulse-core --example netperf -- server
//             cargo run --release -p syspulse-core --example netperf -- <ip> [tcp|udp|latency] [up|down|bidir]
// =============================================================================

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use syspulse_core::netperf::{client, server, Direction, TestKind, TestSpec, DEFAULT_PORT};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("server") {
        let srv = server::start(
            DEFAULT_PORT,
            None,
            Arc::new(|e| println!("{}", serde_json::to_string(&e).unwrap())),
        )
        .unwrap();
        println!(
            "SysPulse speed-test server on {:?}:{} — Ctrl-C to stop",
            srv.info.addresses, DEFAULT_PORT
        );
        loop {
            std::thread::park();
        }
    }
    let target = args.first().cloned().unwrap_or_else(|| "127.0.0.1".into());
    let kind = match args.get(1).map(String::as_str) {
        Some("udp") => TestKind::Udp,
        Some("latency") => TestKind::Latency,
        _ => TestKind::Tcp,
    };
    let direction = match args.get(2).map(String::as_str) {
        Some("down") => Direction::Download,
        Some("bidir") => Direction::Bidir,
        _ => Direction::Upload,
    };
    let spec = TestSpec {
        kind,
        direction,
        streams: 4,
        duration_secs: 5,
        bitrate_mbps: 500.0,
        packet_size: 1400,
        interval_ms: 1000,
        latency_pps: 50,
    };
    let r = client::run_test(
        &target,
        DEFAULT_PORT,
        None,
        &spec,
        &AtomicBool::new(false),
        |p| {
            println!(
                "  {:>8} {:>5.1}s {:>10.1} Mbit/s  retr={:?} rtt={:?} lost={:?} jitter={:?}",
                p.dir, p.t, p.mbps, p.retransmits, p.rtt_ms, p.lost, p.jitter_ms
            )
        },
    )
    .unwrap();
    for d in [&r.upload, &r.download].into_iter().flatten() {
        println!(
            "{}: avg {:.1} Mbit/s (min {:.1} max {:.1} sd {:.1}), {} bytes in {:.2}s, retrans {:?}, rtt {:?}, loss {:?}% jitter {:?} ms ooo {:?}",
            d.direction, d.mbps_avg, d.mbps_min, d.mbps_max, d.mbps_stddev, d.bytes, d.seconds, d.retransmits, d.rtt_ms, d.loss_percent, d.jitter_ms, d.out_of_order
        );
    }
    if let Some(l) = &r.latency {
        println!("latency: min {:.3} avg {:.3} p50 {:.3} p99 {:.3} max {:.3} ms, jitter {:.3}, loss {:.1}% ({}/{})", l.min_ms, l.avg_ms, l.p50_ms, l.p99_ms, l.max_ms, l.jitter_ms, l.loss_percent, l.received, l.sent);
    }
}
