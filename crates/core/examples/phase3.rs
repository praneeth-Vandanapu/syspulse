// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/examples/phase3.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Debug tool for Phase 3 modules.
//           cargo run --release -p syspulse-core --example phase3
// =============================================================================

use std::sync::atomic::AtomicBool;

use syspulse_core::{netconn, search, services, startup};

fn main() {
    let t = std::time::Instant::now();
    let o: search::SearchOptions = serde_json::from_value(serde_json::json!({
        "root": "~", "query": "*.pdf", "mode": "glob", "maxResults": 100000
    }))
    .unwrap();
    let r = search::search(&o, &AtomicBool::new(false), |_| {}).unwrap();
    println!(
        "glob *.pdf in ~: {} hits, {} entries scanned, {} ms",
        r.hits.len(),
        r.scanned,
        r.elapsed_ms
    );

    let o: search::SearchOptions = serde_json::from_value(serde_json::json!({
        "root": "~/UBUNTU_SYSTEM_Monitor", "content": "Praneeth", "extensions": ["rs", "svelte", "ts"]
    }))
    .unwrap();
    let r = search::search(&o, &AtomicBool::new(false), |_| {}).unwrap();
    println!(
        "content 'Praneeth' in project sources: {} files, {} ms",
        r.hits.len(),
        r.elapsed_ms
    );
    if let Some(h) = r.hits.first() {
        println!(
            "  e.g. {} line {:?}",
            h.name,
            h.content.as_ref().map(|c| (c.line, &c.text))
        );
    }

    let s = services::list(false);
    let running = s.iter().filter(|x| x.sub == "running").count();
    let failed: Vec<&str> = s
        .iter()
        .filter(|x| x.active == "failed")
        .map(|x| x.unit.as_str())
        .collect();
    println!(
        "system services: {} ({running} running), failed: {failed:?}",
        s.len()
    );
    println!("user services: {}", services::list(true).len());
    if let Ok(d) = services::details("cron.service", false) {
        println!(
            "cron: pid {:?}, mem {:?}, {} log lines",
            d.main_pid,
            d.memory,
            d.logs.len()
        );
    }

    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap());
    let st = startup::list(&home);
    println!(
        "startup entries: {} ({} visible in GNOME tool)",
        st.len(),
        st.iter().filter(|e| !e.no_display).count()
    );
    for e in st.iter().filter(|e| !e.no_display) {
        println!(
            "  [{}] {} — {}",
            if e.enabled { "on " } else { "off" },
            e.name,
            e.exec
        );
    }

    let n = netconn::list();
    let listen: Vec<String> = n
        .iter()
        .filter(|x| x.state == "LISTEN")
        .map(|x| {
            format!(
                "{}:{} {}",
                x.local_address,
                x.local_port,
                x.process.clone().unwrap_or("?".into())
            )
        })
        .collect();
    println!("sockets: {} total; listening: {listen:?}", n.len());
    println!("total {:?}", t.elapsed());
}
