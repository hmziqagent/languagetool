pub struct MorfologikView(std::sync::Arc<og_tagger::morfologik::MorfologikDictionary>);

impl MorfologikView {
    pub fn new(dictionary: std::sync::Arc<og_tagger::morfologik::MorfologikDictionary>) -> Self {
        Self(dictionary)
    }

    pub fn contains(&self, word: &str) -> bool {
        !self.0.lookup(word).is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
