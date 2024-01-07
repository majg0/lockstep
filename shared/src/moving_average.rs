pub trait MovingAverage {
    fn exponential_moving_average(self, sample: Self, alpha: Self) -> Self;
}

impl MovingAverage for f64 {
    fn exponential_moving_average(self, sample: Self, alpha: Self) -> Self {
        sample * alpha + self * (1.0 - alpha)
    }
}
