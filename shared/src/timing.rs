use std::time::Instant;

pub struct FrameDurationAccumulator {
    pub step_duration: f64,
    pub too_long_step_duration: f64,
    pub current_start_time: Instant,
    pub accumulated_duration: f64,
    pub accumulated_lag: f64,
    pub frame_index: u32,
}

pub struct Frame {
    pub index: u32,
    pub dt: f64,
}

impl FrameDurationAccumulator {
    pub fn with_fps(fps: f64, too_long_step_duration: f64) -> Self {
        FrameDurationAccumulator {
            step_duration: 1.0 / fps,
            too_long_step_duration,
            current_start_time: Instant::now(),
            accumulated_duration: 0.0,
            accumulated_lag: 0.0,
            frame_index: 0,
        }
    }

    pub fn accumulate_duration(&mut self) {
        let start_time = Instant::now();
        let actual_frame_duration = (start_time - self.current_start_time).as_secs_f64();

        // NOTE: when a simulation step takes too long, decrease simulation work
        let provided_frame_duration = actual_frame_duration.min(self.too_long_step_duration);

        // TODO: use lag somehow; maybe we can compensate?
        self.accumulated_lag += actual_frame_duration - provided_frame_duration;
        self.accumulated_duration += provided_frame_duration;
        self.current_start_time = start_time;
    }

    pub fn frame_available(&self) -> bool {
        self.accumulated_duration >= self.step_duration
    }

    pub fn consume_frame(&mut self) {
        self.accumulated_duration -= self.step_duration;
        self.frame_index += 1;
    }

    pub fn interpolate_frames(&self) -> f64 {
        self.accumulated_duration / self.step_duration
    }

    pub fn run_frame<F: FnMut(Frame)>(&mut self, mut run: F) {
        self.accumulate_duration();
        while self.frame_available() {
            run(Frame {
                index: self.frame_index,
                dt: self.step_duration,
            });
            self.consume_frame();
        }
    }
}
