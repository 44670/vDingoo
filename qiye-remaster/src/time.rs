use std::time::Instant;

pub struct FrameTimer {
    pub dt: f32,
    pub elapsed: f32,
    pub tick: u64,
    last: Instant,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            dt: 0.0,
            elapsed: 0.0,
            tick: 0,
            last: Instant::now(),
        }
    }

    pub fn update(&mut self) {
        let now = Instant::now();
        self.dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.elapsed += self.dt;
        self.tick += 1;
    }
}
