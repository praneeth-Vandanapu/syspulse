// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : src-tauri/src/bin/syspulse-capture.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Packet-capture helper for NetLens. Holds the file capabilities
//           cap_net_raw,cap_net_admin (granted once by the user), opens the
//           capture sockets, drops every capability and streams packets to
//           its parent over stdout. Usage:
//               syspulse-capture --check
//               syspulse-capture capture [--promisc] [--snaplen N]
//                                --iface NAME[=BPFHEX] ...
// =============================================================================

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_os = "linux")]
    {
        use syspulse_core::netlens::sniffer;
        let code = match args.first().map(String::as_str) {
            Some("--check") => sniffer::check(),
            Some("capture") => sniffer::run(&args[1..]),
            _ => {
                eprintln!("usage: syspulse-capture --check | capture [--promisc] [--snaplen N] --iface NAME[=BPF] ...");
                2
            }
        };
        std::process::exit(code);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        eprintln!("syspulse-capture is only available on Linux");
        std::process::exit(2);
    }
}
