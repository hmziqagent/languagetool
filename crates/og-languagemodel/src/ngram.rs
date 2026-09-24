pub struct LanguageModel {}

impl LanguageModel {
    pub fn new() -> Self {
        Self {}
    }

    pub fn get_probability(&self, _words: &[&str]) -> f64 {
        0.0
    }
}

impl Default for LanguageModel {
    fn default() -> Self {
        Self::new()
    }
}
