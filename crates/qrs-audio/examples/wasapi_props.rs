//! Dump every string property of every active WASAPI endpoint.
fn main() {
    #[cfg(windows)]
    for (id, props) in qrs_audio::backend::wasapi::dump_properties() {
        println!("== {id}");
        for (k, v) in props {
            println!("   {k} = {v}");
        }
    }
}
