use super::agreement::{PosType, has_reading_of_type, immunized_indices};
use super::helpers::{
    has_pos_tag_starting_with, is_all_uppercase, lowercase_first_char, matches_pos_tag_regex,
    starts_with_uppercase, uppercase_first_char,
};
use og_core::rule::Rule;
use og_core::{
    AnalyzedSentence, AnalyzedToken, AnalyzedTokenReadings, Category, IssueType, RuleMatch,
    RuleMatchContext, RuleMatchRule, SuggestedReplacement,
};
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

const UPPERCASE_MESSAGE: &str =
    "Außer am Satzanfang werden nur Nomen und Eigennamen großgeschrieben.";
const LOWERCASE_MESSAGE: &str =
    "Falls es sich um ein substantiviertes Verb handelt, wird es großgeschrieben.";
const COLON_MESSAGE: &str = "Folgt dem Doppelpunkt weder ein Substantiv noch eine wörtliche Rede oder ein vollständiger Hauptsatz, schreibt man klein weiter.";

static NUMERALS_EN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\A(?:[a-z]|[0-9]+|(m{0,4}(c[md]|d?c{0,3})(x[cl]|l?x{0,3})(i[xv]|v?i{0,3})))\z",
    )
    .unwrap()
});
static TWO_UPPERCASE_CHARS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-ZÖÄÜ][A-ZÖÄÜ][a-zöäüß-]+\z").unwrap());
static VERHALTEN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A.+verhalten\z").unwrap());
static IRGEND_ETC: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A(?:irgendwelche|irgendwas|irgendein|weniger?|einiger?|mehr|aufs)\z")
        .unwrap()
});
static VER_MOD_AUX: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\AVER:(?:MOD|AUX):[1-3]:.*\z").unwrap());
static ALLE_NM: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\AAlle[nm]\z").unwrap());
static IN_INNEN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A(?:in|innen)\z").unwrap());
static SCHAF_PATTERN: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A.{3,}schaf(?:s|en)?\z").unwrap());
static SCHAFE_PATTERN: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A(?:Alpenschaf|Berberschaf|Bergschaf|Blauschaf|Brillenschaf|Dachsteinschaf|Deichschaf|Dickhornschaf|Feinwollschaf|Fettschwanzschaf|Fleischschaf|Fuchsschaf|Glücksschaf|Hausschaf|Jungschaf|Karakulschaf|Klonschaf|Merinoschaf|Milchschaf|Mondschaf|Nutzschaf|Rhönschaf|Riesenwildschaf|Schaukelschaf|Schneeschaf|Steinschaf|Steppenschaf|Superschaf|Waldschaf|Weideschaf|Wildschaf|Wollschaf|Zackelschaf|Zuchtschaf|Zwergblauschaf)(?:s|en)?\z").unwrap()
});
static START_WITH_SPIEL: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\A(?:Spielzugs?|Spielzugangs?|Spielzuganges|Spielzugbuchs?|Spielzugbüchern?|Spielzuges|Spielzugverluste?|Spielzugverluste[ns])\z").unwrap()
});
static END_WITH_SCHAFTE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\A[A-ZÖÄÜ][a-zöäß-]+schafte\z").unwrap());

const NOUN_INDICATORS: [&str; 6] = ["das", "sein", "mein", "dein", "euer", "unser"];

const SENTENCE_START_EXCEPTIONS: [&str; 11] =
    ["(", "\"", "'", "‘", "„", "«", "»", "‚", ".", "!", "?"];

const UNDEFINED_QUANTIFIERS: [&str; 5] = ["viel", "nichts", "nix", "wenig", "allerlei"];

const INTERROGATIVE_PARTICLES: [&str; 9] = [
    "was", "wodurch", "wofür", "womit", "woran", "worauf", "woraus", "wovon", "wie",
];

const POSSESSIVE_INDICATORS: [&str; 6] = ["einer", "eines", "der", "des", "dieser", "dieses"];

const DAS_VERB_EXCEPTIONS: [&str; 7] = ["nur", "sogar", "auch", "die", "alle", "viele", "zu"];

const COLON_QUESTION_WORDS: [&str; 9] = [
    "warum", "wieso", "weshalb", "wer", "was", "wann", "wo", "wie", "wozu",
];

const COLON_QUESTION_CONJUNCTIONS: [&str; 4] = ["und", "oder", "aber", "denn"];

const LANGUAGE_NAMES: [&str; 84] = [
    "Angelsächsisch",
    "Afrikanisch",
    "Albanisch",
    "Altarabisch",
    "Altchinesisch",
    "Altgriechisch",
    "Althochdeutsch",
    "Altpersisch",
    "Amerikanisch",
    "Arabisch",
    "Armenisch",
    "Bairisch",
    "Baskisch",
    "Bengalisch",
    "Bulgarisch",
    "Chinesisch",
    "Dänisch",
    "Deutsch",
    "Englisch",
    "Estnisch",
    "Finnisch",
    "Französisch",
    "Frühneuhochdeutsch",
    "Germanisch",
    "Georgisch",
    "Griechisch",
    "Hebräisch",
    "Hocharabisch",
    "Hochchinesisch",
    "Hochdeutsch",
    "Holländisch",
    "Indonesisch",
    "Irisch",
    "Isländisch",
    "Italienisch",
    "Japanisch",
    "Jiddisch",
    "Jugoslawisch",
    "Kantonesisch",
    "Katalanisch",
    "Klingonisch",
    "Koreanisch",
    "Kroatisch",
    "Kurdisch",
    "Lateinisch",
    "Lettisch",
    "Litauisch",
    "Luxemburgisch",
    "Mittelhochdeutsch",
    "Mongolisch",
    "Neuhochdeutsch",
    "Niederländisch",
    "Norwegisch",
    "Persisch",
    "Plattdeutsch",
    "Polnisch",
    "Portugiesisch",
    "Rätoromanisch",
    "Rumänisch",
    "Russisch",
    "Sächsisch",
    "Schwäbisch",
    "Schwedisch",
    "Schweizerisch",
    "Serbisch",
    "Serbokroatisch",
    "Slawisch",
    "Slowakisch",
    "Slowenisch",
    "Spanisch",
    "Syrisch",
    "Tamilisch",
    "Tibetisch",
    "Tschechisch",
    "Tschetschenisch",
    "Türkisch",
    "Turkmenisch",
    "Uigurisch",
    "Ukrainisch",
    "Ungarisch",
    "Usbekisch",
    "Vietnamesisch",
    "Walisisch",
    "Weißrussisch",
];

static CASE_ANTI_PATTERNS: LazyLock<Option<Vec<crate::engine::XmlPatternRule>>> =
    LazyLock::new(|| {
        let compiler = og_xml::XmlCompiler::new();
        match compiler.compile_file(include_str!("case_antipatterns.xml")) {
            Ok(set) => Some(
                set.rules
                    .into_iter()
                    .map(crate::engine::XmlPatternRule::new)
                    .collect(),
            ),
            Err(_) => None,
        }
    });

static EXCEPTION_PHRASES: LazyLock<Vec<Vec<regex::Regex>>> = LazyLock::new(|| {
    let mut phrases: HashSet<String> = HashSet::new();
    if let Some(lt_root) = crate::engine::find_lt_resource_dir() {
        let dir = lt_root.join("de/src/main/resources/org/languagetool/resource/de");
        for file in ["eigennamen_gross.txt", "case_rule_exceptions.txt"] {
            if let Ok(text) = std::fs::read_to_string(dir.join(file)) {
                for line in text.lines() {
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    phrases.insert(line.to_string());
                }
            }
        }
    }
    phrases
        .into_iter()
        .map(|phrase| {
            phrase
                .split(' ')
                .filter(|p| !p.is_empty())
                .map(|p| regex::Regex::new(&format!(r"\A(?:{p})\z")).unwrap())
                .collect()
        })
        .collect()
});
const EXCEPTIONS: [&str; 667] = [
    "Bedienstete",
    "Bediensteter",
    "Feierwütiger",
    "Feierwütige",
    "Feierwütigen",
    "Dritten",
    "Berufstätige",
    "Berufstätigen",
    "Erwerbstätige",
    "Erwerbstätigen",
    "Tatverdächtige",
    "Tatverdächtigen",
    "Konsumierende",
    "Konsumierenden",
    "Verliebter",
    "Verliebte",
    "Beängstigendes",
    "Oppositioneller",
    "Oppositionelle",
    "Verantwortlicher",
    "Verantwortliche",
    "Verantwortlichen",
    "Beschuldigte",
    "Beschuldigten",
    "Beklagte",
    "Beklagten",
    "Befragte",
    "Befragten",
    "Hingerichtete",
    "Lehrende",
    "Lehrender",
    "Vertrauter",
    "Out",
    "Packet",
    "Paket",
    "Adult",
    "Apart",
    "Universal",
    "Multinational",
    "Additional",
    "Smart",
    "Adverse",
    "Different",
    "Light",
    "Legal",
    "Computational",
    "Holder",
    "Just",
    "Lost",
    "Fundamental",
    "Quick",
    "Infernal",
    "Fit",
    "Fair",
    "Viral",
    "Tough",
    "Indoor",
    "Superb",
    "Resilient",
    "Hexagonal",
    "Responsive",
    "Anno",
    "Mo",
    "Di",
    "Mi",
    "Do",
    "Di. und Do. um 18 Uhr",
    "Fr",
    "Fr. Dr. Müller",
    "Sa",
    "Gr",
    "Gr. 12",
    "Mag",
    "Mag. Helke Müller",
    "Nov",
    "Diss",
    "Invalide",
    "Invalider",
    "Invaliden",
    "Sehende",
    "Sehender",
    "Sehenden",
    "Schutzheilige",
    "Schutzheiliger",
    "Schutzheiligen",
    "Lila",
    "Langzeitarbeitslose",
    "Langzeitarbeitslosen",
    "Langzeitarbeitsloser",
    "Linksintellektuelle",
    "Linksintellektueller",
    "Linksintellektuellen",
    "Beschuldigte",
    "Beschuldigten",
    "Drogenabhängige",
    "Drogenabhängiger",
    "Drogenabhängiger",
    "Drogenabhängigen",
    "Asylsuchender",
    "Asylsuchende",
    "Asylsuchenden",
    "Landtagsabgeordnete",
    "Landtagsabgeordneter",
    "Landtagsabgeordneten",
    "Stadtverordnete",
    "Stadtverordneter",
    "Stadtverordneten",
    "Veränderliche",
    "Veränderlicher",
    "Veränderlichen",
    "Werbetreibende",
    "Werbetreibender",
    "Werbetreibenden",
    "Verletzter",
    "Verletzten",
    "Werktätige",
    "Werktätiger",
    "Werktätigen",
    "Getestete",
    "Getesteten",
    "Genesene",
    "Genesenen",
    "Geimpfte",
    "Geboosterte",
    "Ungeimpfte",
    "Geimpften",
    "Geboosterten",
    "Ungeimpften",
    "Geflüchtete",
    "Geflüchteten",
    "Projektbeteiligte",
    "Projektbeteiligten",
    "Heranwachsende",
    "Heranwachsenden",
    "Interessierte",
    "Interessierten",
    "Infizierte",
    "Infizierten",
    "Gehörlose",
    "Gehörlosen",
    "Drücke",
    "Klecks",
    "Quatsch",
    "Speis",
    "Flash",
    "Suhl",
    "Müh",
    "Bims",
    "Wisch",
    "Außenputz",
    "Rinderhack",
    "Hack",
    "Schlitz",
    "Frevler",
    "Zementputz",
    "Hurst",
    "Bombardier",
    "Kraus",
    "Strunz",
    "Bell",
    "Melk",
    "Klopp",
    "Walz",
    "Schiel",
    "Dusch",
    "Penn",
    "Dörr",
    "Kies",
    "Koks",
    "Dell",
    "Wall",
    "Beige",
    "Zoom",
    "Perl",
    "Parallele",
    "Parallelen",
    "Rutsch",
    "Spar",
    "Merz",
    "Gefahren",
    "Minderjährige",
    "Minderjähriger",
    "Minderjährigen",
    "Scheinselbstständige",
    "Bundestagsabgeordneter",
    "Bundestagsabgeordneten",
    "Bundestagsabgeordnete",
    "Reichstagsabgeordneter",
    "Reichstagsabgeordneten",
    "Reichstagsabgeordnete",
    "Medienschaffende",
    "Medienschaffenden",
    "Medienschaffender",
    "Lehrende",
    "Lehrenden",
    "Vertretene",
    "Vertretenen",
    "Vorstandsvorsitzender",
    "Vorstandsvorsitzenden",
    "Vorstandsvorsitzende",
    "Demonstrierende",
    "Demonstrierenden",
    "Marketingtreibende",
    "Marketingtreibender",
    "Marketingtreibenden",
    "Strafgefangenen",
    "Strafgefangener",
    "Strafgefangene",
    "Pädophile",
    "Pädophiler",
    "Pädophilen",
    "Lehrbeauftragte",
    "Lehrbeauftragter",
    "Lehrbeauftragten",
    "Erkrankte",
    "Erkrankter",
    "Erkrankten",
    "Eigner",
    "Polizeibeamten",
    "Polizeibeamter",
    "Polizeibeamte",
    "Kriegsversehrte",
    "Kriegsversehrter",
    "Kriegsversehrten",
    "Demenzkranke",
    "Demenzkranker",
    "Demenzkranken",
    "Parteivorsitzende",
    "Parteivorsitzender",
    "Parteivorsitzenden",
    "Kriegsgefangene",
    "Kriegsgefangener",
    "Kriegsgefangenen",
    "Ehrenvorsitzende",
    "Ehrenvorsitzender",
    "Ehrenvorsitzenden",
    "Oberkommandierende",
    "Oberkommandierender",
    "Oberkommandierenden",
    "Werbungtreibende",
    "Werbungtreibenden",
    "Mitangeklagte",
    "Schuhfilz",
    "Mix",
    "Rahm",
    "Flansch",
    "WhatsApp",
    "Verschleiß",
    "Schutzsuchende",
    "Schutzsuchenden",
    "Versicherte",
    "Versicherten",
    "Cyberkriminelle",
    "Cyberkriminellen",
    "Kriminelle",
    "Kriminellen",
    "Auszubildenden",
    "Auszubildende",
    "Auszubildender",
    "Lernende",
    "Lernender",
    "Lernenden",
    "Teilnehmende",
    "Teilnehmenden",
    "Radfahrende",
    "Radfahrenden",
    "Autofahrende",
    "Autofahrenden",
    "Auszubildene",
    "Auszubildenen",
    "Absolvierende",
    "Absolvierenden",
    "Einheimische",
    "Einheimischen",
    "Einheimischer",
    "Wehrbeauftragter",
    "Wehrbeauftragte",
    "Wehrbeauftragten",
    "Wehrbeauftragtem",
    "Prozessbevollmächtigter",
    "Prozessbevollmächtigte",
    "Prozessbevollmächtigten",
    "Prozessbevollmächtigtem",
    "Bundesbeamte",
    "Bundesbeamter",
    "Bundesbeamten",
    "Bundesbeamtem",
    "Datenschutzbeauftragter",
    "Datenschutzbeauftragte",
    "Datenschutzbeauftragten",
    "Datenschutzbeauftragtem",
    "Steuerbevollmächtigte",
    "Steuerbevollmächtigter",
    "Steuerbevollmächtigten",
    "Steuerbevollmächtigtem",
    "Suchtkranken",
    "Suchtkranke",
    "Suchtkranker",
    "Filmschaffende",
    "Filmschaffender",
    "Filmschaffenden",
    "Filmschaffendem",
    "Arbeitssuchende",
    "Arbeitssuchender",
    "Arbeitssuchenden",
    "Arbeitssuchendem",
    "Bausachverständige",
    "Bausachverständiger",
    "Bausachverständigen",
    "Bausachverständigem",
    "Heurige",
    "Ratsuchende",
    "Ratsuchender",
    "Ratsuchenden",
    "Verwundete",
    "Verwundeter",
    "Verwundeten",
    "Vollzugsbeamte",
    "Vollzugsbeamter",
    "Vollzugsbeamten",
    "Schutzbefohlene",
    "Schutzbefohlener",
    "Schutzbefohlenen",
    "Verfahrensbeteiligte",
    "Verfahrensbeteiligter",
    "Verfahrensbeteiligten",
    "Kolonialbeamte",
    "Kolonialbeamter",
    "Kolonialbeamten",
    "Verwaltungsbeamte",
    "Verwaltungsbeamter",
    "Verwaltungsbeamten",
    "Verdächtige",
    "Verdächtiger",
    "Verdächtigen",
    "Leichtverletzte",
    "Leichtverletzten",
    "Leichtverletzte",
    "Dozierende",
    "Dozierenden",
    "Studierende",
    "Studierender",
    "Studierenden",
    "Suchbegriffen",
    "Plattdeutsch",
    "Wallet",
    "Str",
    "Priest",
    "Simple",
    "Legend",
    "Golden",
    "Forward",
    "Unverzagt",
    "Auszubildende",
    "Auszubildender",
    "Gelehrte",
    "Gelehrter",
    "Gelehrten",
    "Vorstehende",
    "Vorstehender",
    "Mitwirkende",
    "Mitwirkender",
    "Mitwirkenden",
    "Tabellenletzte",
    "Tabellenletzter",
    "Familienangehörige",
    "Familienangehöriger",
    "Zeitreisende",
    "Zeitreisender",
    "Zeitreisenden",
    "Erwerbstätige",
    "Erwerbstätigen",
    "Erwerbstätiger",
    "Selbstständige",
    "Selbstständigen",
    "Selbstständiger",
    "Selbständige",
    "Selbständigen",
    "Selbständiger",
    "Genaueres",
    "Äußersten",
    "Dienstreisender",
    "Verletzte",
    "Vermisste",
    "Äußeres",
    "Abseits",
    "Unschuldige",
    "Unschuldiger",
    "Unschuldigen",
    "Mitarbeitende",
    "Mitarbeitender",
    "Mitarbeitenden",
    "Beschäftigter",
    "Beschäftigte",
    "Beschäftigten",
    "Beschäftigtem",
    "Bekannter",
    "Bekannte",
    "Bevollmächtigte",
    "Bevollmächtigter",
    "Bevollmächtigten",
    "Brecht",
    "Tel",
    "Unschuldiger",
    "Vorgesetzter",
    "Abs",
    "Klappe",
    "Vorfahre",
    "Mittler",
    "Hr",
    "Schwarz",
    "Genese",
    "Rosa",
    "Auftrieb",
    "Zuschnitt",
    "Geschossen",
    "Vortrieb",
    "Abtrieb",
    "Gesandter",
    "Durchfahrt",
    "Durchgriff",
    "Überfahrt",
    "Zeche",
    "Sparte",
    "Sparten",
    "Heiliger",
    "Reisender",
    "Pest",
    "Schwinge",
    "Verlies",
    "Nachfolge",
    "Stift",
    "Belange",
    "Geistlicher",
    "Google",
    "Hu",
    "Jenseits",
    "Abends",
    "Stimmberechtigte",
    "Stimmberechtigten",
    "Stimmberechtigter",
    "Alleinerziehende",
    "Alleinerziehenden",
    "Alleinerziehender",
    "Abgeordneter",
    "Abgeordnete",
    "Abgeordneten",
    "Angestellter",
    "Angestellte",
    "Angestellten",
    "Armeeangehörige",
    "Armeeangehörigen",
    "Armeeangehöriger",
    "Liberaler",
    "Abriss",
    "Ahne",
    "Ähnlichem",
    "Ähnliches",
    "Er hat Ähnliches erlebt",
    "Allerlei",
    "Anklang",
    "Verlobter",
    "Anstrich",
    "Armes",
    "Ausdrücke",
    "Auswüchsen",
    "Bände",
    "Bänden",
    "Beauftragter",
    "Belange",
    "Biss",
    "De",
    "De Morgan",
    "Diesseits",
    "im Diesseits",
    "Dr",
    "Durcheinander",
    "Eindrücke",
    "Erwachsener",
    "Familienangehörige",
    "Brüder und solche Familienangehörige, die...",
    "Flöße",
    "Folgendes",
    "Fort",
    "Fraß",
    "Fristende",
    "Frevel",
    "Genüge",
    "Gefallen",
    "Gläubige",
    "Gläubiger",
    "Gläubigen",
    "Hechte",
    "Herzöge",
    "Herzögen",
    "Hinfahrt",
    "Hilfsstoff",
    "Hilfsstoffe",
    "Hundert",
    "Zehntausend",
    "Hunderttausend",
    "Hyperwallet",
    "Ihnen",
    "Ihrerseits",
    "Ihr",
    "Ihre",
    "Ihrem",
    "Ihren",
    "Ihrer",
    "Ihres",
    "Infrarot",
    "Jenseits",
    "Jugendliche",
    "Jugendlichen",
    "Jugendlicher",
    "Jünger",
    "Kant",
    "Klaue",
    "Konditional",
    "Krähe",
    "Kurzem",
    "Landwirtschaft",
    "Langem",
    "Längerem",
    "Lausitz",
    "Le",
    "Le Monde",
    "Lehrlingsunterweisung",
    "Leichter",
    "Letzt",
    "Letzt",
    "zu guter Letzt",
    "Letztere",
    "Letzterer",
    "Letzteres",
    "Link",
    "Links",
    "Löhne",
    "Luden",
    "Milk",
    "melken",
    "Mitfahrt",
    "Mr",
    "Mrd",
    "Mrs",
    "Nachfrage",
    "Nachts",
    "des Nachts",
    "eines Nachts",
    "Nachspann",
    "Nähte",
    "Nähten",
    "Narkoseverfahren",
    "Neuem",
    "Neugeborene",
    "Neugeborenen",
    "Neugeborenes",
    "Nr",
    "Nutze",
    "Obdachlose",
    "Obdachloser",
    "Obdachlosen",
    "Oder",
    "Ohrfeige",
    "Patsche",
    "Pfiffe",
    "Pfiffen",
    "Press",
    "Prof",
    "Puste",
    "Sachverständiger",
    "Sankt",
    "Schaulustige",
    "Schaulustigen",
    "Schaulustiger",
    "Scheine",
    "Scheiße",
    "Schuft",
    "Schufte",
    "Schuld",
    "Schwangere",
    "Schwangeren",
    "Schwärme",
    "Schwarzes",
    "Sie",
    "Skype",
    "Spitz",
    "Spott",
    "St",
    "Stereotyp",
    "Störe",
    "Tausend",
    "Tischende",
    "Toter",
    "Übrigen",
    "im Übrigen",
    "Unentschieden",
    "Unvorhergesehenes",
    "etwas Unvorhergesehenes",
    "Verantwortlicher",
    "Verlass",
    "Verwandter",
    "Verstorbenen",
    "Verstorbene",
    "Vielfache",
    "Vielfaches",
    "Vorsitzender",
    "Fraktionsvorsitzender",
    "Verletzte",
    "Verletzten",
    "Walt",
    "Weitem",
    "Weiteres",
    "Wohlen",
    "Wicht",
    "Wichtiges",
    "Wider",
    "das Für und Wider",
    "Wild",
    "Zeche",
    "Zusage",
    "Zwinge",
    "Zirkusrund",
    "Tertiär",
    "Erster",
    "er wurde Erster im Langlauf",
    "Zweiter",
    "Dritter",
    "Vierter",
    "Fünfter",
    "Sechster",
    "Siebter",
    "Achter",
    "Neunter",
    "Erste",
    "sie wurde Erste im Langlauf",
    "Zweite",
    "Dritte",
    "Vierte",
    "Fünfte",
    "Sechste",
    "Siebte",
    "Achte",
    "Neunte",
    "Dein",
    "Deine",
    "Deinem",
    "Deinen",
    "Deiner",
    "Deines",
    "Deinerseits",
    "Dich",
    "Dir",
    "Du",
    "Euch",
    "Euer",
    "Eure",
    "Euern",
    "Eurem",
    "Euren",
    "Eures",
    "Eueren",
    "Euerem",
    "Eueres",
    "Euerer",
    "Eurerseits",
    "Euerseits",
];

const SUBST_VERBEN_EXCEPTIONS: [&str; 37] = [
    "hinziehen",
    "helfen",
    "lassen",
    "passieren",
    "haben",
    "passiert",
    "beschränkt",
    "wiederholt",
    "scheinen",
    "klar",
    "heißen",
    "einen",
    "gehören",
    "bedeutet",
    "ermöglicht",
    "funktioniert",
    "sollen",
    "werden",
    "dürfen",
    "müssen",
    "so",
    "ist",
    "können",
    "mein",
    "sein",
    "muss",
    "muß",
    "wollen",
    "habe",
    "ein",
    "tun",
    "bestätigt",
    "bestätigte",
    "bestätigten",
    "bekommen",
    "sauer",
    "bedeuten",
];

pub struct CaseRule {
    tagger: Option<Arc<og_tagger::GermanTagger>>,
    speller: Option<Arc<og_rules::spelling_support::SpellingSupport>>,
}

impl CaseRule {
    pub fn new(
        tagger: Option<Arc<og_tagger::GermanTagger>>,
        speller: Option<Arc<og_rules::spelling_support::SpellingSupport>>,
    ) -> Self {
        Self { tagger, speller }
    }

    fn lookup(&self, word: &str) -> Option<Vec<AnalyzedToken>> {
        self.tagger.as_deref().and_then(|t| t.lookup(word))
    }

    fn is_misspelled(&self, word: &str) -> bool {
        if SCHAF_PATTERN.is_match(word) && !SCHAFE_PATTERN.is_match(word) {
            let variant = if let Some(stem) = word.strip_suffix("schaf") {
                format!("{stem}schaft")
            } else if let Some(stem) = word.strip_suffix("schafs") {
                format!("{stem}schaft")
            } else if let Some(stem) = word.strip_suffix("schafen") {
                format!("{stem}schaften")
            } else {
                word.to_string()
            };
            if !self.base_is_misspelled(&variant) {
                return true;
            }
        }
        if word.starts_with("Spielzug") && !START_WITH_SPIEL.is_match(word) {
            return true;
        }
        if word.starts_with("Standart")
            && word != "Standarte"
            && word != "Standarten"
            && !word.starts_with("Standartenträger")
            && !word.starts_with("Standartenführer")
        {
            return true;
        }
        if word.ends_with("schafte") && END_WITH_SCHAFTE.is_match(word) {
            return true;
        }
        self.base_is_misspelled(word)
    }

    fn base_is_misspelled(&self, word: &str) -> bool {
        self.speller
            .as_deref()
            .is_some_and(|s| s.is_misspelled(word))
    }

    fn has_noun_reading(&self, readings: Option<&AnalyzedTokenReadings>) -> bool {
        let Some(readings) = readings else {
            return false;
        };
        if has_pos_tag_starting_with(readings, "ABK") && has_partial_pos_tag(readings, "SUB") {
            return true;
        }
        let token = readings.token().token().replace('\u{AD}', "");
        if let Some(all) = self.lookup(&token) {
            for reading in &all {
                for tag in reading.pos_tags() {
                    if tag.contains("SUB:") && !tag.contains(":ADJ") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_number(&self, token: &str) -> bool {
        if !token.is_empty() && token.chars().all(|c| c.is_numeric()) {
            return true;
        }
        self.lookup(&lowercase_first_char(token))
            .is_some_and(|readings| {
                readings
                    .iter()
                    .any(|r| r.pos_tags().iter().any(|t| t == "ZAL"))
            })
    }

    fn is_potential_uppercase_error(
        &self,
        pos: usize,
        tokens: &[&AnalyzedTokenReadings],
        lowercase_readings: Option<&[AnalyzedToken]>,
        is_preceded_by_modal_or_auxiliary: bool,
    ) -> bool {
        if pos <= 1 {
            return false;
        }
        if tokens[pos - 1].token().token() == "zu"
            && !matches_pos_tag_regex(tokens[pos], ".*(NEU|MAS|FEM)$")
            && lowercase_readings.is_some_and(|lc| {
                lc.iter()
                    .any(|r| r.pos_tags().iter().any(|t| t.starts_with("VER:INF")))
            })
        {
            return true;
        }
        if VERHALTEN.is_match(tokens[pos].token().token()) {
            return false;
        }
        let mut is_potential_error = pos + 3 < tokens.len()
            && tokens[pos + 1].token().token() == ","
            && INTERROGATIVE_PARTICLES.contains(&tokens[pos + 2].token().token())
            && has_pos_tag_starting_with(tokens[pos - 1], "VER:MOD")
            && !tokens[pos - 1].has_lemma("mögen")
            && tokens[pos + 3].token().token() != "zum";
        if !is_potential_error
            && lowercase_readings.is_some_and(|lc| {
                lc.iter().any(|r| {
                    r.pos_tags()
                        .iter()
                        .any(|t| t.contains("SUB:NOM:SIN:NEU:INF") || t.contains("SUB:DAT:PLU:"))
                })
            })
            && (tokens[pos - 1].token().token() == "zu"
                || has_partial_tag_opt(
                    Some(tokens[pos - 1]),
                    &["SUB", "EIG", "VER:AUX:3:", "ADV:TMP", "ABK"],
                ))
        {
            let lc = lowercase_readings.unwrap();
            is_potential_error |= lc
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| t == "PA2:PRD:GRU:VER"))
                && !has_pos_tag_starting_with(tokens[pos - 1], "VER:AUX:3")
                && !lc
                    .iter()
                    .any(|r| r.pos_tags().iter().any(|t| t == "VER:3:PLU:PRT:NON"));
            is_potential_error |= (pos + 2 >= tokens.len()
                || tokens[pos + 1].token().token() == ",")
                && (tokens[pos - 1].token().token() == "zu" || is_preceded_by_modal_or_auxiliary)
                && tokens[pos].token().token().starts_with("Über")
                && lc.iter().any(|r| {
                    r.pos_tags()
                        .iter()
                        .any(|t| t.contains("VER:INF:") || t.contains("PA2:PRD:GRU:VER"))
                });
        }
        is_potential_error
    }

    fn potentially_add_lowercase_match(
        &self,
        rule_matches: &mut Vec<RuleMatch>,
        token_readings: &AnalyzedTokenReadings,
        prev_token_is_das: bool,
        token: &str,
        next_token_is_personal_or_reflexive_pronoun: bool,
        sentence: &AnalyzedSentence,
        immunized: bool,
    ) {
        if prev_token_is_das
            && !next_token_is_personal_or_reflexive_pronoun
            && token.chars().next().is_some_and(|c| c.is_lowercase())
            && !SUBST_VERBEN_EXCEPTIONS.contains(&token)
            && has_pos_tag_starting_with(token_readings, "VER:INF")
            && !token_readings.is_ignored_by_speller()
            && !immunized
        {
            add_rule_match(
                rule_matches,
                sentence,
                LOWERCASE_MESSAGE,
                token_readings,
                &uppercase_first_char(token_readings.token().token()),
                self_urls(),
            );
        }
    }

    fn potentially_add_uppercase_match(
        &self,
        rule_matches: &mut Vec<RuleMatch>,
        tokens: &[&AnalyzedTokenReadings],
        i: usize,
        analyzed_token: &AnalyzedTokenReadings,
        token: &str,
        lowercase_readings: Option<&[AnalyzedToken]>,
        sentence: &AnalyzedSentence,
        immunized: &HashSet<usize>,
    ) {
        if i == 0 {
            return;
        }
        let is_upper_first = token.chars().next().is_some_and(|c| c.is_uppercase());
        let lc_word = lowercase_first_char(token);
        if is_upper_first
            && token.chars().count() > 1
            && !analyzed_token.is_ignored_by_speller()
            && !immunized.contains(&i)
            && !SENTENCE_START_EXCEPTIONS.contains(&tokens[i - 1].token().token())
            && !EXCEPTIONS.contains(&token)
            && !is_all_uppercase(token)
            && !self.is_language(i, tokens, token)
            && !self.is_probably_city(i, tokens, token)
            && !has_reading_of_type(analyzed_token, PosType::ProperNoun)
            && !analyzed_token.is_sentence_end()
            && !is_ellipsis(i, tokens)
            && !self.is_numbering(i, tokens)
            && !self.is_nominalization(i, tokens, token, lowercase_readings)
            && !self.is_adverb_and_nominalization(i, tokens)
            && !self.is_special_case(i, tokens)
            && !self.is_adjective_as_noun(i, tokens, lowercase_readings)
            && !self.is_singular_imperative(lowercase_readings, tokens[i])
            && !self.is_exception_phrase(i, tokens)
            && !(i == 2 && tokens[i - 1].token().token() == "\u{201C}")
            && !is_case_typo(token)
            && !followed_by_gender_gap(tokens, i)
            && !is_noun_with_verb_reading(tokens[i])
            && !is_invisible_separator(i.wrapping_sub(1), tokens)
            && !self.is_misspelled(&lc_word)
        {
            if tokens[i - 1].token().token() == ":" {
                if is_question_equivalent_after_colon(i, tokens) {
                    return;
                }
                let subarray = &tokens[..i];
                let is_verb_following = is_verb_following(i, tokens, lowercase_readings);
                if !(is_verb_following || count_pos_tag_starting_with(subarray, "VER") == 0) {
                    add_rule_match(
                        rule_matches,
                        sentence,
                        COLON_MESSAGE,
                        tokens[i],
                        &lc_word,
                        self_urls(),
                    );
                }
                return;
            }
            add_rule_match(
                rule_matches,
                sentence,
                UPPERCASE_MESSAGE,
                tokens[i],
                &lc_word,
                self_urls(),
            );
        }
    }

    fn is_language(&self, i: usize, tokens: &[&AnalyzedTokenReadings], token: &str) -> bool {
        let stripped = if token.ends_with('n') {
            &token[..token.len() - 1]
        } else {
            token
        };
        let stripped = if stripped.ends_with('e') {
            &stripped[..stripped.len() - 1]
        } else {
            stripped
        };
        let maybe_language = (token.ends_with("sch") && LANGUAGE_NAMES.contains(&token))
            || LANGUAGE_NAMES.contains(&stripped);
        let next_readings = tokens.get(i + 1).copied();
        let prev_is_auf = tokens
            .get(i.wrapping_sub(1))
            .is_some_and(|t| t.token().token() == "auf");
        maybe_language && (!self.has_noun_reading(next_readings) || prev_is_auf)
    }

    fn is_probably_city(&self, i: usize, tokens: &[&AnalyzedTokenReadings], token: &str) -> bool {
        if matches!(token, "Klein" | "Groß" | "Neu") {
            let Some(next) = tokens.get(i + 1) else {
                return false;
            };
            let is_tagged = next
                .readings()
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| !t.is_empty()));
            return !is_tagged || has_pos_tag_starting_with(next, "EIG");
        }
        false
    }

    fn is_nominalization(
        &self,
        i: usize,
        tokens: &[&AnalyzedTokenReadings],
        token: &str,
        lowercase_readings: Option<&[AnalyzedToken]>,
    ) -> bool {
        let next_readings = tokens.get(i + 1).copied();
        if starts_with_uppercase(token)
            && !self.is_number(token)
            && !(self.has_noun_reading(next_readings)
                || next_readings.is_some_and(|n| is_numeric(n.token().token())))
            && !ALLE_NM.is_match(token)
        {
            if lowercase_readings.is_some_and(|lc| {
                lc.iter()
                    .any(|r| r.pos_tags().iter().any(|t| t == "PRP:LOK+TMP+CAU:DAT+AKK"))
            }) {
                return false;
            }
            let prev_token = tokens.get(i.wrapping_sub(1)).copied();
            let prev_prev_token = tokens.get(i.wrapping_sub(2)).copied();
            let prev_prev_prev_token = tokens.get(i.wrapping_sub(3)).copied();
            let prev_token_str = prev_token.map(|t| t.token().token()).unwrap_or_default();
            if (matches!(prev_token_str, "und" | "oder" | "beziehungsweise")
                && prev_prev_token.is_some()
                && has_partial_pos_tag(tokens[i], "SUB")
                && has_partial_pos_tag(tokens[i], ":ADJ"))
                || (prev_prev_token.is_some_and(|t| has_partial_pos_tag(t, "SUB"))
                    && !self.has_noun_reading(next_readings)
                    && lowercase_readings.is_some_and(|lc| {
                        lc.iter()
                            .any(|r| r.pos_tags().iter().any(|t| t.contains("ADJ")))
                    })
                    && prev_token_str != ",")
            {
                return true;
            }
            if lowercase_readings.is_some_and(|lc| {
                lc.iter()
                    .any(|r| r.pos_tags().iter().any(|t| t == "PA1:PRD:GRU:VER"))
            }) {
                return false;
            }
            return ((prev_token.is_some()
                && IRGEND_ETC.is_match(prev_token_str)
                && has_partial_pos_tag(tokens[i], "SUB"))
                || self.is_number(prev_token_str))
                || (has_partial_tag_opt(prev_token, &["ART", "PRO:"])
                    && !(((i < 4 && tokens.len() > 4)
                        || prev_token.is_some_and(|t| t.readings().len() == 1)
                        || prev_prev_token.is_some_and(|t| t.has_lemma("sein")))
                        && prev_token
                            .is_some_and(|t| has_pos_tag_starting_with(t, "PRO:PER:NOM:")))
                    && !prev_token.is_some_and(|t| has_partial_pos_tag(t, ":STD")))
                || (has_partial_tag_opt(prev_prev_prev_token, &["ART"])
                    && has_partial_tag_opt(prev_prev_token, &["PRP"])
                    && has_partial_tag_opt(prev_token, &["SUB"]))
                || (has_partial_tag_opt(prev_prev_token, &["PRO:", "PRP"])
                    && has_partial_tag_opt(prev_token, &["ADJ", "ADV", "PA2", "PA1"]))
                || (has_partial_tag_opt(prev_prev_prev_token, &["PRO:", "PRP"])
                    && has_partial_tag_opt(prev_prev_token, &["ADJ", "ADV"])
                    && has_partial_tag_opt(prev_token, &["ADJ", "ADV", "PA2"]))
                || (has_pos_tag_starting_with(tokens[i], "SUB:")
                    && has_partial_tag_opt(prev_token, &["GEN"])
                    && !has_partial_tag_opt(next_readings, &["PKT"]));
        }
        false
    }

    fn is_adverb_and_nominalization(&self, i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
        let prev_prev_token = if i > 1 {
            tokens[i - 2].token().token()
        } else {
            ""
        };
        let prev_token = tokens.get(i.wrapping_sub(1)).copied();
        let token = tokens[i].token().token();
        let next_readings = tokens.get(i + 1).copied();
        prev_prev_token.eq_ignore_ascii_case("das")
            && has_partial_tag_opt(prev_token, &["ADV"])
            && starts_with_uppercase(token)
            && !self.has_noun_reading(next_readings)
    }

    fn is_special_case(&self, i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
        let prev_token = if i > 1 {
            tokens[i - 1].token().token()
        } else {
            ""
        };
        let token = tokens[i].token().token();
        let next_readings = tokens.get(i + 1).copied();
        prev_token.eq_ignore_ascii_case("im")
            && token == "Allgemeinen"
            && !self.has_noun_reading(next_readings)
    }

    fn is_adjective_as_noun(
        &self,
        i: usize,
        tokens: &[&AnalyzedTokenReadings],
        lowercase_readings: Option<&[AnalyzedToken]>,
    ) -> bool {
        let prev_token = tokens.get(i.wrapping_sub(1)).copied();
        let next_readings = tokens.get(i + 1).copied();
        let mut prev_lowercase_readings: Option<Vec<AnalyzedToken>> = None;
        if i > 1 && SENTENCE_START_EXCEPTIONS.contains(&tokens[i - 2].token().token()) {
            if let Some(prev) = prev_token {
                let word = prev.token().token().to_lowercase();
                prev_lowercase_readings = self.lookup(&word);
            }
        }

        let is_possibly_followed_by_infinitive =
            next_readings.is_some_and(|n| n.token().token() == "zu");
        let is_followed_by_infinitive = next_readings
            .is_some_and(|n| !is_possibly_followed_by_infinitive && has_partial_pos_tag(n, "EIZ"));
        let is_followed_by_possessive_indicator =
            next_readings.is_some_and(|n| POSSESSIVE_INDICATORS.contains(&n.token().token()));

        let is_undef_quantifier = prev_token.is_some_and(|p| {
            UNDEFINED_QUANTIFIERS.contains(&p.token().token().to_lowercase().as_str())
        });
        let is_prev_determiner = prev_token.is_some_and(|p| {
            (has_partial_tag_opt(Some(p), &["ART", "PRP", "ZAL"])
                || prev_lowercase_readings
                    .as_deref()
                    .is_some_and(|lc| has_partial_tag_opt_lc(Some(lc), &["ART", "PRP", "ZAL"])))
                && !has_partial_pos_tag(p, ":STD")
        });
        let is_preceded_by_verb = prev_token.is_some_and(|p| {
            matches_pos_tag_regex(p, "VER:(MOD:|AUX:)?[1-3]:.*") && !p.has_lemma("sein")
        });
        if !is_prev_determiner
            && !is_undef_quantifier
            && !(is_possibly_followed_by_infinitive || is_followed_by_infinitive)
            && !(is_preceded_by_verb
                && lowercase_readings.is_some()
                && has_partial_tag_opt_lc(lowercase_readings, &["ADJ:", "PA"])
                && next_readings
                    .is_some_and(|n| !matches!(n.token().token(), "und" | "oder" | ",")))
            && !(is_followed_by_possessive_indicator
                && has_partial_tag_opt_lc(lowercase_readings, &["ADJ", "VER"]))
            && !(prev_token.is_some_and(|p| p.has_pos_tag("KON:UNT"))
                && !self.has_noun_reading(next_readings)
                && next_readings.is_some_and(|n| !n.has_pos_tag("KON:NEB")))
        {
            let prev_prev_token =
                if i > 1 && prev_token.is_some_and(|p| has_partial_pos_tag(p, "ADJ")) {
                    tokens.get(i - 2).copied()
                } else {
                    None
                };
            if !is_preceded_by_verb && lowercase_readings.is_some() && prev_token.is_some() {
                let lc = lowercase_readings.unwrap();
                if has_partial_pos_tag(prev_token.unwrap(), "SUB:")
                    && lc.iter().any(|r| {
                        r.pos_tags().iter().any(|t| {
                            t.starts_with("ADJ:GEN:PLU:MAS:GRU:SOL")
                                || t.starts_with("PA2:GEN:PLU:MAS:GRU:SOL")
                        })
                    })
                {
                    return next_readings.is_some_and(|n| !has_partial_pos_tag(n, "SUB:"));
                } else if next_readings.is_some_and(|n| {
                    n.readings().len() == 1
                        && has_pos_tag_starting_with(prev_token.unwrap(), "PRO:PER:NOM:")
                        && n.has_pos_tag("ADJ:PRD:GRU")
                }) {
                    return true;
                }
            }
            if !has_partial_tag_opt(prev_prev_token, &["ART", "PRP", "ZAL"]) {
                return false;
            }
        }

        for reading in tokens[i].readings() {
            for pos_tag in reading.pos_tags() {
                let tag_empty = pos_tag.is_empty();
                if (tag_empty || pos_tag.contains("ADJ"))
                    && !self.has_noun_reading(next_readings)
                    && !next_readings.is_some_and(|n| is_numeric(n.token().token()))
                {
                    if tag_empty
                        && has_partial_tag_opt_lc(
                            lowercase_readings,
                            &[
                                "PRP:LOK",
                                "PA2:PRD:GRU:VER",
                                "PA1:PRD:GRU:VER",
                                "ADJ:PRD:KOM",
                                "ADV:TMP",
                            ],
                        )
                    {
                        continue;
                    }
                    return true;
                }
            }
        }
        false
    }

    fn is_singular_imperative(
        &self,
        lowercase_readings: Option<&[AnalyzedToken]>,
        token: &AnalyzedTokenReadings,
    ) -> bool {
        lowercase_readings.is_some_and(|lc| {
            lc.iter()
                .any(|r| r.pos_tags().iter().any(|t| t.starts_with("VER:IMP:SIN")))
        }) && token.token().token() != "Ein"
            && token.token().token() != "Eine"
    }

    fn is_exception_phrase(&self, i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
        for patterns in EXCEPTION_PHRASES.iter() {
            for (j, pattern) in patterns.iter().enumerate() {
                if pattern.is_match(tokens[i].token().token()) {
                    let start_index = i as isize - j as isize;
                    if compare_lists(
                        tokens,
                        start_index,
                        start_index + patterns.len() as isize - 1,
                        patterns,
                    ) {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn is_numbering(&self, i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
        i >= 2
            && matches!(tokens[i - 1].token().token(), ")" | "]")
            && NUMERALS_EN.is_match(tokens[i - 2].token().token())
            && !(i > 3
                && tokens[i - 3].token().token() == "("
                && has_pos_tag_starting_with(tokens[i - 4], "SUB:"))
    }
}

fn self_urls() -> Vec<String> {
    vec![
        "https://dict.leo.org/grammatik/deutsch/Rechtschreibung/Regeln/Gross-klein/index.html"
            .to_string(),
    ]
}

fn add_rule_match(
    rule_matches: &mut Vec<RuleMatch>,
    sentence: &AnalyzedSentence,
    msg: &str,
    token_readings: &AnalyzedTokenReadings,
    fixed_word: &str,
    urls: Vec<String>,
) {
    let from = token_readings.token().start();
    let to = token_readings.token().end();
    let rm_rule = RuleMatchRule::new(
        "DE_CASE",
        "Großschreibung von Nomen und substantivierten Verben",
    )
    .with_category(Category::new("CASING", "Groß-/Kleinschreibung"))
    .with_issue_type("casing")
    .with_urls(urls);
    rule_matches.push(
        RuleMatch::new(
            msg,
            from,
            to - from,
            rm_rule,
            RuleMatchContext::new(sentence.text().to_string(), from, to - from),
        )
        .with_sentence(sentence.text().to_string())
        .with_replacements(vec![SuggestedReplacement::new(fixed_word)]),
    );
}

fn has_partial_pos_tag(token: &AnalyzedTokenReadings, needle: &str) -> bool {
    token
        .readings()
        .iter()
        .any(|r| r.pos_tags().iter().any(|t| t.contains(needle)))
}

fn has_partial_tag_opt(token: Option<&AnalyzedTokenReadings>, pos_tags: &[&str]) -> bool {
    token.is_some_and(|t| pos_tags.iter().any(|p| has_partial_pos_tag(t, p)))
}

fn has_partial_tag_opt_lc(readings: Option<&[AnalyzedToken]>, pos_tags: &[&str]) -> bool {
    readings.is_some_and(|lc| {
        lc.iter().any(|r| {
            r.pos_tags()
                .iter()
                .any(|t| pos_tags.iter().any(|p| t.contains(p)))
        })
    })
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_numeric())
}

fn count_pos_tag_starting_with(tokens: &[&AnalyzedTokenReadings], prefix: &str) -> usize {
    tokens
        .iter()
        .filter(|t| has_pos_tag_starting_with(t, prefix))
        .count()
}

fn is_salutation(token: &str) -> bool {
    matches!(token, "Herr" | "Hr" | "Herrn" | "Frau" | "Fr" | "Fräulein")
}

fn is_company(token: &str) -> bool {
    matches!(
        token,
        "Firma" | "Familie" | "Unternehmen" | "Firmen" | "Bäckerei" | "Metzgerei" | "Fa"
    )
}

fn is_prev_probably_relative_pronoun(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    i >= 3
        && tokens[i - 1].token().token() == "das"
        && tokens[i - 2].token().token() == ","
        && matches_pos_tag_regex(tokens[i - 3], "SUB:...:SIN:NEU")
}

fn is_followed_by_relative_or_subordinate_clause(
    i: usize,
    tokens: &[&AnalyzedTokenReadings],
) -> bool {
    if i + 4 < tokens.len() {
        return tokens[i + 1].token().token() == ","
            && (INTERROGATIVE_PARTICLES.contains(&tokens[i + 2].token().token())
                || tokens[i + 2].has_pos_tag("KON:UNT"));
    }
    false
}

fn is_case_typo(token: &str) -> bool {
    TWO_UPPERCASE_CHARS.is_match(token)
}

fn followed_by_gender_gap(tokens: &[&AnalyzedTokenReadings], i: usize) -> bool {
    i + 2 < tokens.len()
        && tokens[i + 1].token().token() == ":"
        && IN_INNEN.is_match(tokens[i + 2].token().token())
}

fn is_noun_with_verb_reading(token: &AnalyzedTokenReadings) -> bool {
    has_pos_tag_starting_with(token, "SUB") && has_pos_tag_starting_with(token, "VER:INF")
}

fn is_invisible_separator(i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
    tokens
        .get(i)
        .is_some_and(|t| t.token().token().starts_with('\u{2063}'))
}

fn is_ellipsis(i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
    matches!(tokens[i - 1].token().token(), "]" | ")")
        && ((i == 4 && tokens[i - 2].token().token() == "…")
            || (i == 6 && tokens[i - 2].token().token() == "."))
}

fn is_verb_following(
    i: usize,
    tokens: &[&AnalyzedTokenReadings],
    lowercase_readings: Option<&[AnalyzedToken]>,
) -> bool {
    let first_is_verb = match lowercase_readings {
        Some(lc) => lc
            .iter()
            .any(|r| r.pos_tags().iter().any(|t| t.starts_with("VER:"))),
        None => has_pos_tag_starting_with(tokens[i], "VER:"),
    };
    let mut count = usize::from(first_is_verb);
    for t in &tokens[i + 1..] {
        if has_pos_tag_starting_with(t, "VER:") {
            count += 1;
        }
    }
    count != 0
}

fn is_colon_question_word(word: &str) -> bool {
    COLON_QUESTION_WORDS
        .iter()
        .any(|w| w.eq_ignore_ascii_case(word))
}

fn is_question_equivalent_after_colon(i: usize, tokens: &[&AnalyzedTokenReadings]) -> bool {
    if i + 1 < tokens.len() {
        let word = tokens[i].token().token();
        let next = tokens[i + 1].token().token();
        if is_colon_question_word(word) && next == "?" {
            return true;
        }
        if COLON_QUESTION_CONJUNCTIONS
            .iter()
            .any(|c| c.eq_ignore_ascii_case(word))
            && i + 2 < tokens.len()
            && is_colon_question_word(tokens[i + 1].token().token())
            && tokens[i + 2].token().token() == "?"
        {
            return true;
        }
    }
    false
}

fn compare_lists(
    tokens: &[&AnalyzedTokenReadings],
    start_index: isize,
    end_index: isize,
    patterns: &[regex::Regex],
) -> bool {
    if start_index < 0 {
        return false;
    }
    let mut i = 0usize;
    let mut j = start_index;
    while j <= end_index {
        if i >= patterns.len()
            || j >= tokens.len() as isize
            || !patterns[i].is_match(tokens[j as usize].token().token())
        {
            return false;
        }
        i += 1;
        j += 1;
    }
    true
}

impl Rule for CaseRule {
    fn id(&self) -> &str {
        "DE_CASE"
    }

    fn description(&self) -> &str {
        "Großschreibung von Nomen und substantivierten Verben"
    }

    fn is_default_on(&self) -> bool {
        true
    }

    fn category(&self) -> Category {
        Category::new("CASING", "Groß-/Kleinschreibung")
    }

    fn issue_type(&self) -> IssueType {
        IssueType::Other("casing".to_string())
    }

    fn match_sentence(&self, sentence: &AnalyzedSentence) -> Vec<RuleMatch> {
        let tokens = sentence.non_whitespace_tokens();
        let immunized: HashSet<usize> = CASE_ANTI_PATTERNS
            .as_ref()
            .map(|rules| immunized_indices(rules, sentence))
            .unwrap_or_default();
        let mut rule_matches = Vec::new();
        let mut prev_token_is_das = false;
        let mut is_preceded_by_modal_or_auxiliary = false;
        for i in 0..tokens.len() {
            if first_pos_tag(tokens[i]).is_some_and(|t| t == "SENT_START") {
                continue;
            }
            if i == 1 {
                prev_token_is_das =
                    NOUN_INDICATORS.contains(&tokens[1].token().token().to_lowercase().as_str());
                continue;
            }
            if i > 0
                && (is_salutation(tokens[i - 1].token().token())
                    || is_company(tokens[i - 1].token().token()))
            {
                continue;
            }
            if i > 2
                && NUMERALS_EN.is_match(tokens[i - 1].token().token())
                && tokens[i - 2].token().token() == "."
                && NUMERALS_EN.is_match(tokens[i - 3].token().token())
            {
                continue;
            }

            let analyzed_token = tokens[i];
            let token = analyzed_token.token().token();
            let is_baseform =
                analyzed_token.readings().len() >= 1 && analyzed_token.has_lemma(token);
            if (first_pos_tag(analyzed_token).is_none()
                || has_reading_of_type(analyzed_token, PosType::Verb))
                && is_baseform
            {
                let mut next_token_is_personal_or_reflexive_pronoun = false;
                if i + 1 < tokens.len() {
                    let next = tokens[i + 1];
                    next_token_is_personal_or_reflexive_pronoun =
                        has_partial_pos_tag(next, "PRO:PER")
                            || matches!(next.token().token(), "sich" | "Sie");
                    if next.has_pos_tag("PKT") {
                        continue;
                    }
                    if prev_token_is_das
                        && (DAS_VERB_EXCEPTIONS.contains(&next.token().token())
                            || is_followed_by_relative_or_subordinate_clause(i, &tokens))
                        || (i > 1
                            && has_partial_tag_opt(
                                tokens.get(i - 2).copied(),
                                &["VER:AUX", "VER:MOD"],
                            ))
                    {
                        continue;
                    }
                }
                if is_prev_probably_relative_pronoun(&tokens, i)
                    || (prev_token_is_das && count_pos_tag_starting_with(&tokens, "VER") == 1)
                {
                    continue;
                }
                self.potentially_add_lowercase_match(
                    &mut rule_matches,
                    tokens[i],
                    prev_token_is_das,
                    token,
                    next_token_is_personal_or_reflexive_pronoun,
                    sentence,
                    immunized.contains(&i),
                );
            }
            prev_token_is_das = NOUN_INDICATORS.contains(&token.to_lowercase().as_str());
            if analyzed_token
                .readings()
                .iter()
                .any(|r| r.pos_tags().iter().any(|t| VER_MOD_AUX.is_match(t)))
            {
                is_preceded_by_modal_or_auxiliary = true;
            }
            let lowercase_readings = self.lookup(&token.to_lowercase());
            if self.has_noun_reading(Some(analyzed_token)) {
                if !self.is_potential_uppercase_error(
                    i,
                    &tokens,
                    lowercase_readings.as_deref(),
                    is_preceded_by_modal_or_auxiliary,
                ) {
                    continue;
                }
            } else if has_pos_tag_starting_with(analyzed_token, "SUB:")
                && i + 1 < tokens.len()
                && tokens[i + 1]
                    .token()
                    .token()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase())
                && matches_pos_tag_regex(tokens[i + 1], "(VER:[123]:|PA2).+")
            {
                continue;
            }
            if first_pos_tag(analyzed_token).is_none() && lowercase_readings.is_none() {
                continue;
            }
            if first_pos_tag(analyzed_token).is_none()
                && let Some(lc) = lowercase_readings.as_deref()
                && (first_pos_tag_lc(lc).is_none() || token.ends_with("innen"))
            {
                continue;
            }
            self.potentially_add_uppercase_match(
                &mut rule_matches,
                &tokens,
                i,
                analyzed_token,
                token,
                lowercase_readings.as_deref(),
                sentence,
                &immunized,
            );
        }
        rule_matches
    }
}

fn first_pos_tag(token: &AnalyzedTokenReadings) -> Option<&str> {
    token
        .readings()
        .first()
        .and_then(|r| r.pos_tags().first())
        .map(String::as_str)
}

fn first_pos_tag_lc(readings: &[AnalyzedToken]) -> Option<&str> {
    readings
        .first()
        .and_then(|r| r.pos_tags().first())
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use crate::engine::LanguageEngine;

    fn shared_engine() -> &'static LanguageEngine {
        crate::engine::LanguageEngine::shared_for_code("de").expect("german engine")
    }

    fn case_matches(text: &str) -> Vec<(String, usize, String)> {
        shared_engine()
            .check(text)
            .matches
            .iter()
            .filter(|m| m.rule().id() == "DE_CASE")
            .map(|m| {
                (
                    m.message().to_string(),
                    m.offset(),
                    m.replacements()
                        .first()
                        .map(|r| r.value().to_string())
                        .unwrap_or_default(),
                )
            })
            .collect()
    }

    #[test]
    fn flags_mid_sentence_adjective() {
        let matches = case_matches("Ich habe Heute keine Zeit.");
        assert!(
            matches
                .iter()
                .any(|(msg, _, sugg)| msg.contains("großgeschrieben") && sugg == "heute"),
            "got {matches:?}"
        );
    }

    #[test]
    fn flags_substantivized_verb_lowercase() {
        let matches = case_matches("Das laufen fällt mir leicht.");
        assert!(
            matches
                .iter()
                .any(|(msg, _, sugg)| msg.contains("substantiviertes Verb") && sugg == "Laufen"),
            "got {matches:?}"
        );
    }

    #[test]
    fn accepts_correct_sentences() {
        for text in [
            "Das Laufen fällt mir leicht.",
            "Ein einfacher Satz zum Testen.",
            "Heute spricht Frau Stieg.",
            "3.1 Technische Dokumentation",
            "Er arbeitet im Bereich Präsidiales.",
            "Anmeldung bis Fr. 1.12.",
            "Eine Gruppe Aufständischer verwüstete die Bar.",
            "Das Winseln stört.",
            "Hier ein Satz. \"Ein Zitat.\"",
        ] {
            let matches = case_matches(text);
            assert!(matches.is_empty(), "{text}: got {matches:?}");
        }
    }
}
