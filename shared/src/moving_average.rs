pub trait MovingAverage {
    fn exponential_moving_average(&mut self, sample: Self, alpha: Self);
}

impl MovingAverage for f64 {
    fn exponential_moving_average(&mut self, sample: Self, alpha: Self) {
        *self = sample * alpha + *self * (1.0 - alpha)
    }
}
