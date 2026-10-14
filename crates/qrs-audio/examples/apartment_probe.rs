//! How ASIO driver enumeration behaves across threads and COM apartments: lists the ASIO devices once per
//! argument, each from a new thread initialised as a multithreaded ("mta") or single-threaded ("sta") apartment,
//! not initialised ("none"), or from the main thread ("here"). Lists only (no streams).
//!
//! Found 2026-10-14 with the SDK called directly: "mta mta" lost the driver on the second thread, as did
//! "here mta" and "sta mta"; "sta"/"none"/"here" always worked. `backend::asio::enumerate` now lists from a
//! thread of its own, so every order finds the driver.
//!
//!     cargo run --release -p qrs-audio --features asio --example apartment_probe -- mta here mta none

fn list(tag: &str) {
    let devs = qrs_audio::backend::enumerate_only(qrs_audio::backend::Backend::Asio);
    println!(
        "{tag}: {} ASIO device(s): {:?}",
        devs.len(),
        devs.iter().map(|d| &d.name).collect::<Vec<_>>()
    );
}

fn main() {
    let order: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    for (i, kind) in order.iter().enumerate() {
        let kind = kind.clone();
        let tag = format!("{i} {kind}");
        let same = kind == "here";
        let f = move || {
            match kind.as_str() {
                "sta" => unsafe {
                    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                },
                "mta" => qrs_audio::backend::wasapi::com_init(),
                _ => {}
            }
            list(&tag);
        };
        if same {
            f();
        } else {
            std::thread::spawn(f).join().unwrap();
        }
    }
}
