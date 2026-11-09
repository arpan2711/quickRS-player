fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/rt/pitch_shift.cpp");
    println!("cargo:rerun-if-changed=vendor/stftpitchshift/LICENSE");
    println!("cargo:rerun-if-changed=vendor/stftpitchshift/cpp/StftPitchShift");

    cc::Build::new()
        .cpp(true)
        .std("c++20")
        .opt_level(2)
        .include("vendor/stftpitchshift/cpp")
        .file("src/rt/pitch_shift.cpp")
        .compile("quickrs_stft_pitchshift");
}
