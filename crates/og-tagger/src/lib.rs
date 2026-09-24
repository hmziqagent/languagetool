pub mod chunker;
pub mod disambiguator;
pub mod english_tagger;
pub mod german_chunker;
pub mod german_tagger;
pub mod morfologik;
pub mod multiword_chunker;
pub mod opennlp;
pub mod tagger;
pub mod xml_disambiguator;

pub use disambiguator::*;
pub use english_tagger::EnglishTagger;
pub use german_chunker::GermanChunker;
pub use german_tagger::GermanTagger;
pub use multiword_chunker::{MultiWordChunker, MultiWordChunkerDisambiguator};
pub use opennlp::OpenNlpEnglishChunker;
pub use tagger::*;
pub use xml_disambiguator::XmlDisambiguator;

use og_core::AnalyzedTokenReadings;

pub trait Tagger: Send + Sync {
    fn tag(&self, tokens: &[&str]) -> Vec<AnalyzedTokenReadings>;
}
