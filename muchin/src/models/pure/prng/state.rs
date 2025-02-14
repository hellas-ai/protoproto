use rand::{rngs::SmallRng, SeedableRng};

#[allow(dead_code)]
pub struct PRNGConfig {
    pub seed: u64,
}

#[derive(Debug)]
pub struct PRNGState {
    pub rng: SmallRng,
}

#[allow(dead_code)]
impl PRNGState {
    pub fn from_config(config: PRNGConfig) -> Self {
        Self {
            rng: SmallRng::seed_from_u64(config.seed),
        }
    }

    pub fn seed(&mut self, seed: u64) {
        self.rng = SmallRng::seed_from_u64(seed)
    }
}
