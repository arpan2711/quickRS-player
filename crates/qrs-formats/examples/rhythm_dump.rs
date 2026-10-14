//! For tools/parity/rhythm_ref.py: the rhythm of every part of the given songs, one line per part.
//!   cargo run --release -p qrs-formats --example rhythm_dump -- KEYS.json SONG...
use qrs_formats::rhythm::{Options, for_arrangement};
use qrs_formats::{Package, load_keys};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let keys = load_keys(Some(std::path::Path::new(&a[1])), &[]).unwrap();
    for path in &a[2..] {
        let Ok(mut pkg) = Package::open(path, &keys, None) else {
            continue;
        };
        for info in pkg.arrangements.clone() {
            let Ok(arr) = pkg.load_info(&info, 1.0) else {
                continue;
            };
            let r = for_arrangement(&arr, Options::default());
            println!("{}\t{}\t{}", path, info.key, r.describe(f64::MIN, f64::MAX));
        }
    }
}
