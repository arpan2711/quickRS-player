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
constexpr double kPitchRampSeconds = 0.02;

class MonitorShifter {
public:
    MonitorShifter(std::uint32_t rate, float semitones)
        : fft_(std::make_shared<stftpitchshift::RFFT>()),
          stft_(fft_, std::make_tuple(kAnalysisSize, kBlock), kHop),
          core_(fft_, std::make_tuple(kAnalysisSize, kBlock), kHop, rate),
          factors_{std::pow(2.0, static_cast<double>(semitones) / 12.0)},
          current_semitones_(semitones),
          target_semitones_(semitones),
          ramp_length_(std::max<std::size_t>(1, static_cast<std::size_t>(std::ceil(
              rate * kPitchRampSeconds / kHop)))),
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

    void set_semitones(float semitones) noexcept {
        if (!std::isfinite(semitones)) {
            return;
        }
        const double target = std::clamp(static_cast<double>(semitones), -12.0, 12.0);
        if (target != target_semitones_) {
            target_semitones_ = target;
            ramp_frames_ = ramp_length_;
        }
    }

    void process(const float* input, float* output) noexcept {
        // Retain the analysis history, then append one fresh host block.
        std::rotate(input_.begin(), input_.begin() + kBlock, input_.end());
        std::copy_n(input, kBlock, input_.begin() + kAnalysisSize);

        // The synthesis buffer carries overlap from the previous block. The STFT adds this
        // block's frames into it; the next block takes over the remaining overlap.
        stft_(input_, output_, [this](std::span<std::complex<float>> spectrum) {
            if (ramp_frames_ > 0) {
                current_semitones_ += (target_semitones_ - current_semitones_) / ramp_frames_;
                --ramp_frames_;
                // factors_ and the core's one-factor buffers were sized at construction, so this update allocates
                // nothing on the audio callback.
                factors_[0] = std::pow(2.0, current_semitones_ / 12.0);
                core_.factors(factors_);
            }
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
    double current_semitones_;
    double target_semitones_;
    std::size_t ramp_frames_ = 0;
    std::size_t ramp_length_;
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

void quickrs_monitor_shifter_set_semitones(void* shifter, float semitones) noexcept {
    if (shifter != nullptr) {
        static_cast<MonitorShifter*>(shifter)->set_semitones(semitones);
    }
}

void quickrs_monitor_shifter_delete(void* shifter) noexcept {
    delete static_cast<MonitorShifter*>(shifter);
}

} // extern "C"
