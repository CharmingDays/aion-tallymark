//! Passive packet capture.
//!
//! Npcap hands this process a copy of TCP segments that already arrived on the
//! machine. Nothing is injected, and the game process is never opened.

use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::decode::{self, Event};
use crate::demo::now_ms;
use crate::reassembly::Assembler;
use crate::death_debug::DeathDebug;
use crate::spawn_debug::SpawnDebug;
use crate::state::AppState;
use crate::target_debug::TargetDebug;

struct Segment {
    src: IpAddr,
    sport: u16,
    dst: IpAddr,
    dport: u16,
    seq: u32,
    payload: Vec<u8>,
}

/// `debug` is the opt-in unnamed-target log. `None` leaves decoding untouched.
pub fn start(state: Arc<AppState>, debug: Option<TargetDebug>) {
    let (tx, rx) = sync_channel::<Segment>(4096);
    let worker = state.clone();
    thread::Builder::new()
        .name("aion2dps-decode".into())
        .spawn(move || decode_loop(rx, worker, debug))
        .expect("decode thread");
    #[cfg(windows)]
    spawn_capture(tx, state);
    #[cfg(not(windows))]
    {
        let _ = tx;
        state.with_capture(|capture| {
            capture.running = false;
            capture.error = Some(
                "Packet capture in this build runs on Windows with Npcap. The meter pages still work with --demo.".into(),
            );
        });
    }
}

fn decode_loop(rx: Receiver<Segment>, state: Arc<AppState>, mut debug: Option<TargetDebug>) {
    let mut assembler = Assembler::new();
    let mut spawns = SpawnDebug::from_env();
    let mut deaths = DeathDebug::from_env();
    let mut cube_discovery = crate::cube_discovery::CubeDiscovery::from_env();
    let mut cube_flow = None;
    while let Ok(segment) = rx.recv() {
        state.with_capture(|capture| capture.segments = capture.segments.saturating_add(1));
        let framed = assembler.push(
            segment.src,
            segment.sport,
            segment.dst,
            segment.dport,
            segment.seq,
            &segment.payload,
        );
        if framed.is_empty() {
            continue;
        }
        let now = now_ms();
        for frame in framed {
            let events = decode::parse_packet(&frame.packet);
            let locks = events.iter().any(|event| match event {
                Event::Hit(hit) => !hit.taken && hit.total() > 0 && hit.skill >= 10_000_000,
                _ => false,
            });
            // Trust only the direction identified by self identity or combat,
            // never a coincidental opcode on another captured TCP connection.
            let flow = (segment.src, segment.sport, segment.dst, segment.dport);
            let identifies_server = events.iter().any(|event| matches!(event,
                Event::You { .. } | Event::Name { is_self: true, .. }))
                || locks;
            if identifies_server && cube_flow != Some(flow) {
                state.with_cubes(|cubes| cubes.reset_world());
                cube_flow = Some(flow);
            }
            state.with_cubes(|cubes| {
                let found = if cube_flow == Some(flow) {
                    cubes.observe_packet(&frame.packet, &events, now)
                } else {
                    crate::hidden_cubes::observations(&frame.packet, &cubes.protocol)
                };
                if let Some(log) = cube_discovery.as_mut() {
                    log.observe(&frame.packet, &found, cubes, now, cube_flow == Some(flow));
                }
            });
            if let Some(spawns) = spawns.as_mut() {
                spawns.observe(&frame.packet, &events, now);
            }
            if let Some(deaths) = deaths.as_mut() {
                deaths.observe(&frame.packet, &events, now);
            }
            if locks {
                assembler.lock_port(frame.src_port);
            }
            let hit_count = events
                .iter()
                .filter(|event| matches!(event, Event::Hit(_)))
                .count() as u64;
            let Some(debug) = debug.as_mut() else {
                state.with_combat(|combat| {
                    for event in events {
                        combat.apply(event, now);
                    }
                });
                state.with_capture(|capture| {
                    capture.frames = capture.frames.saturating_add(1);
                    capture.hits = capture.hits.saturating_add(hit_count);
                    if locks {
                        capture.locked_port = Some(frame.src_port);
                    }
                });
                continue;
            };
            // Names and types are checked after this packet's own records apply.
            let (named, typed): (Vec<i32>, Vec<(i32, u32)>) = state.with_combat(|combat| {
                for event in events.iter().cloned() {
                    combat.apply(event, now);
                }
                let ids: Vec<i32> = events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Hit(hit) => Some(hit.target),
                        Event::Hp { id, .. } => Some(*id),
                        _ => None,
                    })
                    .collect();
                let named = ids.iter().copied().filter(|id| combat.knows_name(*id)).collect();
                let typed = ids
                    .iter()
                    .filter_map(|id| Some((*id, combat.monster_type(*id)?)))
                    .collect();
                (named, typed)
            });
            debug.observe(
                &frame.packet,
                &events,
                |id| named.contains(&id),
                |id| typed.iter().find(|(seen, _)| *seen == id).map(|(_, t)| *t),
                now,
            );
            let info = debug.info();
            state.with_capture(|capture| {
                capture.frames = capture.frames.saturating_add(1);
                capture.hits = capture.hits.saturating_add(hit_count);
                if locks {
                    capture.locked_port = Some(frame.src_port);
                }
                capture.target_debug = Some(info);
            });
        }
    }
}

#[cfg(windows)]
fn spawn_capture(tx: SyncSender<Segment>, state: Arc<AppState>) {
    match open_devices() {
        Ok(devices) if devices.is_empty() => state.with_capture(|capture| {
            capture.error = Some(
                "Npcap did not list a capture device. Install Npcap with WinPcap API-compatible mode, then run aion2dps as Administrator.".into(),
            );
        }),
        Ok(devices) => {
            let names: Vec<String> = devices.iter().map(|d| d.description.clone()).collect();
            state.with_capture(|capture| {
                capture.running = true;
                capture.devices = names;
                capture.error = None;
            });
            let running = Arc::new(AtomicBool::new(true));
            for device in devices {
                let tx = tx.clone();
                let state = state.clone();
                let running = running.clone();
                thread::Builder::new()
                    .name(format!("aion2dps-{}", device.name))
                    .spawn(move || capture_device(&device.name, tx, state, running))
                    .ok();
            }
        }
        Err(err) => state.with_capture(|capture| {
            capture.error = Some(err);
        }),
    }
}

#[cfg(windows)]
struct Device {
    name: String,
    description: String,
}

#[cfg(windows)]
fn open_devices() -> Result<Vec<Device>, String> {
    let lib = load_npcap()?;
    let mut err = [0i8; 256];
    let mut all = std::ptr::null_mut();
    let rc = unsafe { (lib.findalldevs)(&mut all, err.as_mut_ptr()) };
    if rc != 0 {
        return Err(format!(
            "Npcap could not list adapters ({}). Install Npcap with WinPcap API-compatible mode and run as Administrator.",
            cstr(&err)
        ));
    }
    let mut devices = Vec::new();
    let mut cursor = all;
    while !cursor.is_null() {
        unsafe {
            let name = cstr_ptr((*cursor).name);
            let description = cstr_ptr((*cursor).description);
            if !name.is_empty() {
                devices.push(Device {
                    description: if description.is_empty() {
                        name.clone()
                    } else {
                        description
                    },
                    name,
                });
            }
            cursor = (*cursor).next;
        }
    }
    unsafe { (lib.freealldevs)(all) };
    Ok(devices)
}

#[cfg(windows)]
fn capture_device(
    name: &str,
    tx: SyncSender<Segment>,
    state: Arc<AppState>,
    running: Arc<AtomicBool>,
) {
    let lib = match load_npcap() {
        Ok(lib) => lib,
        Err(err) => {
            state.with_capture(|capture| capture.error = Some(err));
            return;
        }
    };
    let mut err = [0i8; 256];
    let c_name =
        std::ffi::CString::new(name).unwrap_or_else(|_| std::ffi::CString::new("").unwrap());
    let handle = unsafe { (lib.create)(c_name.as_ptr(), err.as_mut_ptr()) };
    if handle.is_null() {
        tracing::warn!("skipping adapter {name}: {}", cstr(&err));
        return;
    }
    unsafe {
        (lib.set_snaplen)(handle, 65535);
        (lib.set_promisc)(handle, 0);
        (lib.set_timeout)(handle, 100);
        (lib.set_buffer_size)(handle, 16 * 1024 * 1024);
        if let Some(set_immediate) = lib.set_immediate {
            set_immediate(handle, 1);
        }
        let activated = (lib.activate)(handle);
        if activated < 0 {
            let message = cstr_ptr((lib.geterr)(handle));
            state.with_capture(|capture| {
                if capture.error.is_none() {
                    capture.error = Some(format!(
                        "Could not open {name}: {message}. Install Npcap with WinPcap API-compatible mode and run as Administrator."
                    ));
                }
            });
            (lib.close)(handle);
            return;
        }
        let filter = std::ffi::CString::new("tcp and not port 80 and not port 443").unwrap();
        let mut program = BpfProgram {
            len: 0,
            insns: std::ptr::null_mut(),
        };
        if (lib.compile)(handle, &mut program, filter.as_ptr(), 1, 0) == 0 {
            let _ = (lib.setfilter)(handle, &mut program);
            (lib.freecode)(&mut program);
        }
    }

    while running.load(Ordering::Relaxed) {
        let mut header = std::ptr::null_mut();
        let mut data = std::ptr::null();
        let rc = unsafe { (lib.next_ex)(handle, &mut header, &mut data) };
        if rc == 0 {
            continue;
        }
        if rc < 0 {
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        if header.is_null() || data.is_null() {
            continue;
        }
        let caplen = unsafe { (*header).caplen as usize };
        let bytes = unsafe { std::slice::from_raw_parts(data, caplen) };
        if let Some(segment) = tcp_segment(bytes) {
            if let Err(TrySendError::Full(_)) = tx.try_send(segment) {
                state.with_capture(|capture| capture.drops = capture.drops.saturating_add(1));
            }
        }
    }
    unsafe { (lib.close)(handle) };
}

#[cfg(windows)]
fn tcp_segment(frame: &[u8]) -> Option<Segment> {
    use etherparse::{NetSlice, SlicedPacket, TransportSlice};
    let sliced = SlicedPacket::from_ethernet(frame)
        .ok()
        .or_else(|| SlicedPacket::from_ip(frame).ok())?;
    let TransportSlice::Tcp(tcp) = sliced.transport? else {
        return None;
    };
    let payload = tcp.payload();
    if payload.is_empty() {
        return None;
    }
    let (src, dst) = match sliced.net? {
        NetSlice::Ipv4(v4) => (
            IpAddr::V4(v4.header().source_addr()),
            IpAddr::V4(v4.header().destination_addr()),
        ),
        NetSlice::Ipv6(v6) => (
            IpAddr::V6(v6.header().source_addr()),
            IpAddr::V6(v6.header().destination_addr()),
        ),
        NetSlice::Arp(_) => return None,
    };
    Some(Segment {
        src,
        sport: tcp.source_port(),
        dst,
        dport: tcp.destination_port(),
        seq: tcp.sequence_number(),
        payload: payload.to_vec(),
    })
}

#[cfg(windows)]
fn cstr(buf: &[i8]) -> String {
    let bytes = buf
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as u8)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

#[cfg(windows)]
fn cstr_ptr(ptr: *const i8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_string_lossy()
        .trim()
        .to_string()
}

#[cfg(windows)]
#[repr(C)]
struct PcapIf {
    next: *mut PcapIf,
    name: *mut i8,
    description: *mut i8,
    addresses: *mut std::ffi::c_void,
    flags: u32,
}

#[cfg(windows)]
#[repr(C)]
struct PcapPkthdr {
    ts_sec: i32,
    ts_usec: i32,
    caplen: u32,
    len: u32,
}

#[cfg(windows)]
#[repr(C)]
struct BpfProgram {
    len: u32,
    insns: *mut std::ffi::c_void,
}

/// Where users get Npcap. The UI opens this page; the app never downloads it.
pub const NPCAP_URL: &str = "https://npcap.com";

/// Shown in the capture status when wpcap.dll cannot be loaded. The pages key
/// their "Get Npcap" button off the word Npcap, so keep it in this text.
pub const NPCAP_MISSING: &str = "Npcap was not found. aion2dps needs Npcap for live capture. Install it from https://npcap.com with 'WinPcap API-compatible Mode' checked, then run aion2dps as Administrator.";

#[cfg(windows)]
struct Npcap {
    findalldevs: unsafe extern "C" fn(*mut *mut PcapIf, *mut i8) -> i32,
    freealldevs: unsafe extern "C" fn(*mut PcapIf),
    create: unsafe extern "C" fn(*const i8, *mut i8) -> *mut std::ffi::c_void,
    set_snaplen: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32,
    set_promisc: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32,
    set_timeout: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32,
    set_buffer_size: unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32,
    set_immediate: Option<unsafe extern "C" fn(*mut std::ffi::c_void, i32) -> i32>,
    activate: unsafe extern "C" fn(*mut std::ffi::c_void) -> i32,
    compile:
        unsafe extern "C" fn(*mut std::ffi::c_void, *mut BpfProgram, *const i8, i32, u32) -> i32,
    setfilter: unsafe extern "C" fn(*mut std::ffi::c_void, *mut BpfProgram) -> i32,
    freecode: unsafe extern "C" fn(*mut BpfProgram),
    next_ex:
        unsafe extern "C" fn(*mut std::ffi::c_void, *mut *mut PcapPkthdr, *mut *const u8) -> i32,
    geterr: unsafe extern "C" fn(*mut std::ffi::c_void) -> *const i8,
    close: unsafe extern "C" fn(*mut std::ffi::c_void),
}

#[cfg(windows)]
fn load_npcap() -> Result<&'static Npcap, String> {
    use std::sync::OnceLock;
    static LOADED: OnceLock<Result<Npcap, String>> = OnceLock::new();
    LOADED
        .get_or_init(|| {
            let wide: Vec<u16> = "C:\\Windows\\System32\\Npcap\0".encode_utf16().collect();
            unsafe {
                windows_sys::Win32::System::LibraryLoader::SetDllDirectoryW(wide.as_ptr());
            }
            let library = unsafe { libloading::Library::new("wpcap.dll") }.map_err(|_| {
                NPCAP_MISSING.to_string()
            })?;
            let library = Box::leak(Box::new(library));
            Ok(Npcap {
                findalldevs: load_symbol(library, b"pcap_findalldevs")?,
                freealldevs: load_symbol(library, b"pcap_freealldevs")?,
                create: load_symbol(library, b"pcap_create")?,
                set_snaplen: load_symbol(library, b"pcap_set_snaplen")?,
                set_promisc: load_symbol(library, b"pcap_set_promisc")?,
                set_timeout: load_symbol(library, b"pcap_set_timeout")?,
                set_buffer_size: load_symbol(library, b"pcap_set_buffer_size")?,
                set_immediate: load_symbol(library, b"pcap_set_immediate_mode").ok(),
                activate: load_symbol(library, b"pcap_activate")?,
                compile: load_symbol(library, b"pcap_compile")?,
                setfilter: load_symbol(library, b"pcap_setfilter")?,
                freecode: load_symbol(library, b"pcap_freecode")?,
                next_ex: load_symbol(library, b"pcap_next_ex")?,
                geterr: load_symbol(library, b"pcap_geterr")?,
                close: load_symbol(library, b"pcap_close")?,
            })
        })
        .as_ref()
        .map_err(|err| err.clone())
}

#[cfg(windows)]
fn load_symbol<T: Copy>(library: &'static libloading::Library, name: &[u8]) -> Result<T, String> {
    let mut terminated = name.to_vec();
    terminated.push(0);
    unsafe { library.get::<T>(&terminated) }
        .map(|symbol| *symbol)
        .map_err(|err| {
            format!(
                "Npcap is missing {} ({err}). Reinstall Npcap with WinPcap API-compatible mode.",
                String::from_utf8_lossy(name)
            )
        })
}

#[cfg(test)]
mod npcap_message_tests {
    use super::{NPCAP_MISSING, NPCAP_URL};

    #[test]
    fn missing_npcap_message_is_actionable() {
        assert!(NPCAP_MISSING.starts_with("Npcap was not found."));
        assert!(NPCAP_MISSING.contains(NPCAP_URL));
        assert!(NPCAP_MISSING.contains("WinPcap API-compatible Mode"));
        assert!(NPCAP_MISSING.contains("Administrator"));
        assert_eq!(NPCAP_URL, "https://npcap.com");
    }
}