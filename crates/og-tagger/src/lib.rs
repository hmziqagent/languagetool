pub mod chunker;
pub mod disambiguator;
pub mod english_data;
pub mod english_tagger;
pub mod tagger;
pub mod xml_disambiguator;

pub use disambiguator::*;
pub use english_tagger::EnglishTagger;
pub use tagger::*;
pub use xml_disambiguator::XmlDisambiguator;

use og_core::AnalyzedTokenReadings;

pub trait Tagger: Send + Sync {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings>;
}
