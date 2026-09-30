use og_test_runner::runner::TestRunner;

fn main() {
    let runner = TestRunner::for_language("it");
    for text in ["Io le ritrovo quando osservo", "Quanto costa lo vino?"] {
        println!("\n'{text}':");
        let sentence = runner.debug_sentence(text);
        for (i, token) in sentence.non_whitespace_tokens().iter().enumerate() {
            println!("  [{i}] '{}'", token.token().token());
            for r in token.readings() {
                println!(
                    "       {} {}",
                    r.lemma().unwrap_or("-"),
                    r.pos_tags().join(",")
                );
            }
        }
    }
}
