#include <algorithm>
#include <cmath>
#include <complex>
#include <cstddef>
#include <cstdint>
#include <memory>
#include <span>
#include <tuple>
#include <vector>

#include <StftPitchShift/RFFT.h>
#include <StftPitchShift/STFT.h>
#include <StftPitchShift/StftPitchShiftCore.h>

namespace {

// The audio callback feeds this exact block size. The asymmetric STFT uses a long analysis
// window but only a short synthesis window, keeping the output delay tied to this block.
constexpr std::size_t kBlock = 128;
// The longer analysis frame resolves low guitar fundamentals and reduces the glassy/metallic quality. Output latency
// remains tied to kBlock because the synthesis window stays short and asymmetric.
constexpr std::size_t kDftSize = 4096;
constexpr std::size_t kAnalysisSize = 2 * kDftSize;
constexpr std::size_t kOverlap = 4;
constexpr std::size_t kHop = kBlock / kOverlap;

class MonitorShifter {
public:
    MonitorShifter(std::uint32_t rate, float semitones)
        : fft_(std::make_shared<stftpitchshift::RFFT>()),
          stft_(fft_, std::make_tuple(kAnalysisSize, kBlock), kHop),
          core_(fft_, std::make_tuple(kAnalysisSize, kBlock), kHop, rate),
          factors_{std::pow(2.0, static_cast<double>(semitones) / 12.0)},
          input_(kAnalysisSize + kBlock, 0.0f),
          output_(kAnalysisSize + kBlock, 0.0f),
          warm_time_(kAnalysisSize, 0.0f),
          warm_frequency_(kAnalysisSize / 2 + 1) {
        core_.factors(factors_);
        // Keep the spectral envelope while shifting the harmonics; a 1 ms quefrency is short relative to guitar
        // fundamentals and helps avoid the thin, metallic quality of a shifted tone.
        core_.quefrency(0.001);
        // RFFT lazily creates its tables on first use. Force that control-thread allocation
        // before this object is handed to the realtime callback.
        fft_->fft(warm_time_, warm_frequency_);
    }

    void process(const float* input, float* output) noexcept {
        // Retain the analysis history, then append one fresh host block.
        std::rotate(input_.begin(), input_.begin() + kBlock, input_.end());
        std::copy_n(input, kBlock, input_.begin() + kAnalysisSize);

        // The synthesis buffer carries overlap from the previous block. The STFT adds this
        // block's frames into it; the next block takes over the remaining overlap.
        stft_(input_, output_, [this](std::span<std::complex<float>> spectrum) {
            core_.shiftpitch(spectrum);
        });

        std::copy_n(output_.begin() + kAnalysisSize - kBlock, kBlock, output);
        std::rotate(output_.begin(), output_.begin() + kBlock, output_.end());
        std::fill(output_.end() - kBlock, output_.end(), 0.0f);
    }

private:
    std::shared_ptr<stftpitchshift::RFFT> fft_;
    stftpitchshift::STFT<float> stft_;
    stftpitchshift::StftPitchShiftCore<float> core_;
    std::vector<double> factors_;
    std::vector<float> input_;
    std::vector<float> output_;
    std::vector<float> warm_time_;
    std::vector<std::complex<float>> warm_frequency_;
};

} // namespace

extern "C" {

std::size_t quickrs_monitor_shifter_block_size() noexcept {
    return kBlock;
}

void* quickrs_monitor_shifter_new(std::uint32_t rate, float semitones) noexcept {
    try {
        return new MonitorShifter(rate, semitones);
    } catch (...) {
        return nullptr;
    }
}

void quickrs_monitor_shifter_process(void* shifter, const float* input, float* output) noexcept {
    if (shifter != nullptr && input != nullptr && output != nullptr) {
        static_cast<MonitorShifter*>(shifter)->process(input, output);
    }
}

void quickrs_monitor_shifter_delete(void* shifter) noexcept {
    delete static_cast<MonitorShifter*>(shifter);
}

} // extern "C"
