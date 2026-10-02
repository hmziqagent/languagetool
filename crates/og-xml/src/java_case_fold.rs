use regex_syntax::ast::parse::Parser;
use regex_syntax::ast::print::Printer;
use regex_syntax::ast::{
    Ast, ClassBracketed, ClassSet, ClassSetItem, ClassSetUnion, Literal, LiteralKind, Span,
};

const DOTLESS_I: char = '\u{0131}';
const DOTTED_CAPITAL_I: char = '\u{0130}';

pub fn compile_java_ci(pattern: &str) -> Option<regex::Regex> {
    let rewritten = rewrite_java_ci(pattern);
    let source = rewritten.as_deref().unwrap_or(pattern);
    regex::RegexBuilder::new(source)
        .case_insensitive(true)
        .build()
        .ok()
        .or_else(|| {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .ok()
        })
}

fn rewrite_java_ci(pattern: &str) -> Option<String> {
    if pattern.contains("?-i") {
        return None;
    }
    let mut ast = Parser::new().parse(pattern).ok()?;
    rewrite_ast(&mut ast)?;
    let mut out = String::with_capacity(pattern.len() + 8);
    Printer::new().print(&ast, &mut out).ok()?;
    Some(out)
}

fn rewrite_ast(ast: &mut Ast) -> Option<()> {
    match ast {
        Ast::Empty(_)
        | Ast::Flags(_)
        | Ast::Dot(_)
        | Ast::Assertion(_)
        | Ast::ClassUnicode(_)
        | Ast::ClassPerl(_) => Some(()),
        Ast::Literal(lit) => match lit.c {
            'i' => {
                *ast = bracketed(&lit.span, &['i', DOTLESS_I, DOTTED_CAPITAL_I]);
                Some(())
            }
            'I' => {
                *ast = bracketed(&lit.span, &['I', DOTLESS_I]);
                Some(())
            }
            _ => Some(()),
        },
        Ast::ClassBracketed(cls) => rewrite_class(cls),
        Ast::Repetition(rep) => rewrite_ast(&mut rep.ast),
        Ast::Group(group) => rewrite_ast(&mut group.ast),
        Ast::Alternation(alt) => alt.asts.iter_mut().try_for_each(rewrite_ast),
        Ast::Concat(cat) => cat.asts.iter_mut().try_for_each(rewrite_ast),
    }
}

fn rewrite_class(cls: &mut ClassBracketed) -> Option<()> {
    let span = cls.span;
    let items = match &mut cls.kind {
        ClassSet::BinaryOp(_) => return None,
        ClassSet::Item(ClassSetItem::Union(union)) => &mut union.items,
        ClassSet::Item(single) => {
            let existing = std::mem::replace(
                single,
                ClassSetItem::Union(ClassSetUnion {
                    span,
                    items: Vec::new(),
                }),
            );
            match single {
                ClassSetItem::Union(union) => {
                    union.span = span;
                    union.items.push(existing);
                    &mut union.items
                }
                _ => return None,
            }
        }
    };
    for item in items.iter() {
        if matches!(item, ClassSetItem::Bracketed(_)) {
            return None;
        }
    }
    let has_i = items_contain(items, 'i');
    let has_upper_i = items_contain(items, 'I');
    if has_i {
        items.push(literal_item(span, DOTLESS_I));
        items.push(literal_item(span, DOTTED_CAPITAL_I));
    } else if has_upper_i {
        items.push(literal_item(span, DOTLESS_I));
    }
    Some(())
}

fn items_contain(items: &[ClassSetItem], c: char) -> bool {
    items.iter().any(|item| match item {
        ClassSetItem::Literal(lit) => lit.c == c,
        ClassSetItem::Range(range) => range.start.c <= c && c <= range.end.c,
        _ => false,
    })
}

fn literal_item(span: Span, c: char) -> ClassSetItem {
    ClassSetItem::Literal(Literal {
        span,
        kind: LiteralKind::Verbatim,
        c,
    })
}

fn bracketed(span: &Span, chars: &[char]) -> Ast {
    let items = chars
        .iter()
        .map(|&c| literal_item(*span, c))
        .collect::<Vec<_>>();
    Ast::ClassBracketed(Box::new(ClassBracketed {
        span: *span,
        negated: false,
        kind: ClassSet::Item(ClassSetItem::Union(ClassSetUnion { span: *span, items })),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ci(pattern: &str) -> regex::Regex {
        compile_java_ci(pattern).expect("pattern must compile")
    }

    #[test]
    fn ascii_case_folding_is_unchanged() {
        let re = ci("^(?:[a-z]+)$");
        assert!(re.is_match("Word"));
        assert!(!re.is_match("Word2"));
    }

    #[test]
    fn dotless_i_matches_lower_and_upper_ascii_i_classes_like_java() {
        let lower = ci("^(?:[a-z]+)$");
        assert!(
            lower.is_match("fayız"),
            "Java CASE_INSENSITIVE|UNICODE_CASE folds ı into [a-z]"
        );
        let upper = ci("^(?:[A-Z]+)$");
        assert!(upper.is_match("FAYIZ"));
        assert!(!upper.is_match("FAYİZ"), "İ does not fold into [A-Z]");
    }

    #[test]
    fn dotted_capital_i_folds_into_lower_i_class_only() {
        let lower = ci("^(?:h[i]gh)$");
        assert!(lower.is_match("hıgh"));
        assert!(lower.is_match("hİgh"));
        assert!(lower.is_match("high"));
        assert!(!lower.is_match("hogh"));
    }

    #[test]
    fn negated_class_excludes_java_fold_partners() {
        let neg_lower = ci("^(?:[^a-z]+)$");
        assert!(neg_lower.is_match("123"));
        assert!(!neg_lower.is_match("FAYIZ"));
        assert!(!neg_lower.is_match("FAYİZ"));
        let neg_upper = ci("^(?:[^A-Z]+)$");
        assert!(!neg_upper.is_match("ı"));
        assert!(
            neg_upper.is_match("İ"),
            "İ stays outside the [A-Z] fold orbit"
        );
    }

    #[test]
    fn plain_literal_i_matches_all_four_i_forms() {
        let lit = ci("^(?:big)$");
        assert!(lit.is_match("big"));
        assert!(lit.is_match("BIG"));
        assert!(lit.is_match("bıg"));
        assert!(lit.is_match("bİg"));
    }

    #[test]
    fn class_without_ascii_i_keeps_its_fold_orbit() {
        let re = ci("^(?:[a-hx-z]+)$");
        assert!(re.is_match("abc"));
        assert!(!re.is_match("ı"));
        assert!(!re.is_match("ijk"));
    }

    #[test]
    fn crh_complex_number_regexp_matches_dotless_i_token() {
        let pattern = r"^(?:[$+-]?[0-9,-]*[0-9,][A-Za-zñğüşöçâ][A-Za-zñğüşöçâ-]*[.²³]?)$";
        let re = ci(pattern);
        assert!(re.is_match("21fayız"));
        assert!(re.is_match("21fayiz"));
        assert!(!re.is_match("21"));
    }

    #[test]
    fn case_sensitive_flag_group_disables_rewrite() {
        assert_eq!(rewrite_java_ci(r"^(?:a(?-i:i)b)$"), None);
    }

    #[test]
    fn leading_bounded_repeat_and_alternation_still_compile() {
        let re = ci("^(?:hi|bye){1,3}$");
        assert!(re.is_match("hibye"));
        assert!(re.is_match("hıbye"));
    }
}
