use std::ffi::c_void;
use std::ptr::NonNull;

const BLOCK: usize = 128;

unsafe extern "C" {
    fn quickrs_monitor_shifter_block_size() -> usize;
    fn quickrs_monitor_shifter_new(rate: u32, semitones: f32) -> *mut c_void;
    fn quickrs_monitor_shifter_process(shifter: *mut c_void, input: *const f32, output: *mut f32);
    fn quickrs_monitor_shifter_set_semitones(shifter: *mut c_void, semitones: f32);
    fn quickrs_monitor_shifter_delete(shifter: *mut c_void);
}

/// A preconfigured monitor shifter, including its callback-safe block adapter.
pub struct MonitorPitchShifter {
    handle: NonNull<c_void>,
    input: [f32; BLOCK],
    output: [f32; BLOCK],
    filled: usize,
    output_at: usize,
    output_ready: bool,
}

unsafe impl Send for MonitorPitchShifter {}

impl MonitorPitchShifter {
    /// Construct off the audio callback; processing and buffer swaps allocate nothing.
    pub(crate) fn new(rate: u32, semitones: f32) -> Option<Self> {
        if unsafe { quickrs_monitor_shifter_block_size() } != BLOCK {
            return None;
        }
        let handle = NonNull::new(unsafe { quickrs_monitor_shifter_new(rate, semitones) })?;
        Some(Self {
            handle,
            input: [0.0; BLOCK],
            output: [0.0; BLOCK],
            filled: 0,
            output_at: 0,
            output_ready: false,
        })
    }

    /// Adapt arbitrary callback sizes in place without allocating.
    pub(crate) fn process(&mut self, samples: &mut [f32]) {
        for sample in samples {
            let input = *sample;
            *sample = if self.output_ready {
                let output = self.output[self.output_at];
                self.output_at += 1;
                if self.output_at == BLOCK {
                    self.output_at = 0;
                    self.output_ready = false;
                }
                output
            } else {
                0.0
            };

            self.input[self.filled] = input;
            self.filled += 1;
            if self.filled == BLOCK {
                unsafe {
                    quickrs_monitor_shifter_process(
                        self.handle.as_ptr(),
                        self.input.as_ptr(),
                        self.output.as_mut_ptr(),
                    );
                }
                self.filled = 0;
                self.output_ready = true;
            }
        }
    }

    pub(crate) fn set_semitones(&mut self, semitones: f32) {
        unsafe { quickrs_monitor_shifter_set_semitones(self.handle.as_ptr(), semitones) };
    }
}

impl Drop for MonitorPitchShifter {
    fn drop(&mut self) {
        unsafe { quickrs_monitor_shifter_delete(self.handle.as_ptr()) };
    }
}
