# stftPitchShift

Vendored C++ headers used by the live monitor pitch shifter. Upstream project:
<https://github.com/jurihock/stftPitchShift>

Pinned upstream commit: `70c96db087458f8631edbc1bb4068d42ea5b52cc`.
The copied source is MIT-licensed; see `LICENSE`.

quickRS uses `StftPitchShiftCore`, `STFT`, and `RFFT` behind its own small C ABI
wrapper. The separate JUCE plugin repository is not included.
