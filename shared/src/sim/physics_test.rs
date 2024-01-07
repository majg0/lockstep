use crate::net::stream::{Stream, Streamable};

pub struct PhysicsTest {
    pub position: f64,
    pub velocity: f64,
    pub acceleration: f64,
    pub drag: f64,
}

impl PhysicsTest {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            position: 0.0,
            velocity: 0.0,
            acceleration: 1.0,
            drag: 0.1,
        }
    }

    pub fn simulate(&mut self, dt: f64) {
        self.acceleration *= self.drag * dt;
        self.velocity += self.acceleration * dt;
        self.position += self.velocity * dt;
    }
}

impl Streamable for PhysicsTest {
    fn stream<S: Stream>(&mut self, stream: &mut S) {
        self.position.stream(stream);
        self.velocity.stream(stream);
        self.acceleration.stream(stream);
    }
}
