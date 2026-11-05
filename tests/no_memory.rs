//! The meter must stay a packet listener. These names are the Win32 and pcap
//! calls that would read a process or inject a packet.

#[test]
fn source_never_reads_process_memory_or_injects_packets() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let banned = [
        "ReadProcessMemory",
        "WriteProcessMemory",
        "NtReadVirtualMemory",
        "OpenProcess",
        "VirtualQueryEx",
        "process_vm_readv",
        "pcap_sendpacket",
        "pcap_inject",
    ];
    let mut offenders = Vec::new();
    visit(&manifest.join("src"), &banned, &mut offenders);
    visit(&manifest.join("src-tauri").join("src"), &banned, &mut offenders);
    assert!(
        offenders.is_empty(),
        "memory or inject APIs found:\n{}",
        offenders.join("\n")
    );
}

fn visit(dir: &std::path::Path, banned: &[&str], offenders: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, banned, offenders);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for name in banned {
            if text.contains(name) {
                offenders.push(format!("{} contains {name}", path.display()));
            }
        }
    }
}
