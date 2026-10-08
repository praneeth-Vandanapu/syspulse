// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : crates/core/src/netlens/wireshark.rs
//  Author : Praneeth
//  License: MIT
//  Purpose: Optional bridge to an installed Wireshark (the "hybrid" part of
//           NetLens): decode a single frame with tshark's 3000+ dissectors
//           (`tshark -V` text tree parsed by indentation), and open the
//           current capture in the Wireshark GUI. Wireshark is never needed
//           for NetLens' own features.
// =============================================================================

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;

use super::dissect::Node;
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WiresharkTools {
    pub tshark: Option<String>,
    pub wireshark: Option<String>,
}

fn which(name: &str) -> Option<PathBuf> {
    ["/usr/bin", "/usr/local/bin", "/bin", "/snap/bin"]
        .iter()
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

pub fn tools() -> WiresharkTools {
    WiresharkTools {
        tshark: which("tshark").map(|p| p.to_string_lossy().to_string()),
        wireshark: which("wireshark").map(|p| p.to_string_lossy().to_string()),
    }
}

/// Parse `tshark -V` output (4 spaces per level) into a tree.
pub fn parse_verbose(text: &str) -> Vec<Node> {
    fn node(label: &str) -> Node {
        Node {
            label: label.to_string(),
            field: "",
            filter: String::new(),
            start: 0,
            len: 0,
            level: 0,
            children: Vec::new(),
        }
    }
    let mut roots: Vec<Node> = Vec::new();
    // stack of (depth, node)
    let mut stack: Vec<(usize, Node)> = Vec::new();
    let flush_to = |stack: &mut Vec<(usize, Node)>, roots: &mut Vec<Node>, depth: usize| {
        while stack.last().is_some_and(|(d, _)| *d >= depth) {
            let (_, n) = stack.pop().unwrap_or((0, node("")));
            match stack.last_mut() {
                Some((_, parent)) => parent.children.push(n),
                None => roots.push(n),
            }
        }
    };
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let depth = indent / 4;
        flush_to(&mut stack, &mut roots, depth);
        let mut n = node(line.trim());
        if n.label.starts_with("[Expert Info") {
            n.level = 2;
        }
        stack.push((depth, n));
    }
    flush_to(&mut stack, &mut roots, 0);
    roots
}

/// Decode one frame (already written to `pcap`) with tshark.
pub fn decode(pcap: &Path) -> Result<Vec<Node>> {
    let tshark = which("tshark").ok_or(CoreError::Unsupported("Wireshark decoding (install the tshark package)"))?;
    let mut child = Command::new(tshark)
        .arg("-r")
        .arg(pcap)
        .arg("-V")
        .arg("-n")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Other(format!("cannot run tshark: {e}")))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(_status) = child.try_wait()? {
            break;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            return Err(CoreError::Other("tshark took too long".into()));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return Err(CoreError::Other(format!(
            "tshark failed: {}",
            String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("no output")
        )));
    }
    Ok(parse_verbose(&text))
}

/// Open a capture file in the Wireshark GUI (detached).
pub fn open_gui(file: &Path) -> Result<()> {
    let ws = which("wireshark").ok_or(CoreError::Unsupported("Open in Wireshark (Wireshark is not installed)"))?;
    let mut child = Command::new(ws)
        .arg("-r")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| CoreError::Other(format!("cannot start Wireshark: {e}")))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_indent_tree() {
        let t = "Frame 1: 60 bytes\n    Arrival Time: x\n    [Protocols: eth]\nEthernet II\n    Destination: a\n        Address: a\n    Source: b\n";
        let n = parse_verbose(t);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].children.len(), 2);
        assert_eq!(n[1].children[0].children[0].label, "Address: a");
        assert_eq!(n[1].children[1].label, "Source: b");
    }
}
