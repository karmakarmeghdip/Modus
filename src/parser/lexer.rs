//! Modus Lexer implementation using Chumsky.

use super::token::Token;
use chumsky::prelude::*;

pub fn lexer<'src>()
-> impl Parser<'src, &'src str, Vec<(Token, SimpleSpan)>, extra::Err<Rich<'src, char>>> {
    // Comments
    let line_comment = just("//")
        .then(any().and_is(just('\n').not()).repeated())
        .ignored();

    let block_comment = just("/*")
        .then(any().and_is(just("*/").not()).repeated())
        .then(just("*/"))
        .ignored();

    let comment = line_comment.or(block_comment).padded();

    // Numbers: floats must be attempted before integers
    let float = text::digits(10)
        .then(just('.'))
        .then(text::digits(10))
        .to_slice()
        .map(|s: &str| Token::Float(s.parse::<f64>().unwrap()));

    let int = text::digits(10)
        .to_slice()
        .map(|s: &str| Token::Int(s.parse::<i64>().unwrap()));

    let number = float.or(int);

    // Strings with basic escape support
    let str_escape = just('\\').ignore_then(choice((
        just('\\').to('\\'),
        just('/').to('/'),
        just('"').to('"'),
        just('b').to('\x08'),
        just('f').to('\x0C'),
        just('n').to('\n'),
        just('r').to('\r'),
        just('t').to('\t'),
    )));

    let str_content = none_of("\"\\")
        .or(str_escape)
        .repeated()
        .collect::<String>();
    let string = just('"')
        .ignore_then(str_content)
        .then_ignore(just('"'))
        .map(Token::Str);

    // Multi-character symbols
    let multi_sym = choice((
        just("=>").to(Token::Arrow),
        just("...").to(Token::Spread),
        just("==").to(Token::EqEq),
        just("!=").to(Token::BangEq),
        just("<=").to(Token::LtEq),
        just(">=").to(Token::GtEq),
        just("&&").to(Token::AndAnd),
        just("||").to(Token::OrOr),
    ));

    // Single-character symbols
    let single_sym = choice((
        just('+').to(Token::Plus),
        just('-').to(Token::Minus),
        just('*').to(Token::Star),
        just('/').to(Token::Slash),
        just('%').to(Token::Percent),
        just('<').to(Token::Lt),
        just('>').to(Token::Gt),
        just('!').to(Token::Bang),
        just('=').to(Token::Eq),
        just('|').to(Token::Pipe),
        just(':').to(Token::Colon),
        just(';').to(Token::Semi),
        just(',').to(Token::Comma),
        just('.').to(Token::Dot),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
        just('[').to(Token::LBracket),
        just(']').to(Token::RBracket),
    ));

    // Identifiers and Keywords: allows [a-zA-Z_][a-zA-Z0-9_]*
    let ident_start = any().filter(|c: &char| c.is_ascii_alphabetic() || *c == '_');
    let ident_continue = any().filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_');
    let ident = ident_start
        .then(ident_continue.repeated())
        .to_slice()
        .map(|s: &str| match s {
            "function" => Token::Function,
            "type" => Token::Type,
            "let" => Token::Let,
            "if" => Token::If,
            "else" => Token::Else,
            "match" => Token::Match,
            "trait" => Token::Trait,
            "impl" => Token::Impl,
            "perform" => Token::Perform,
            "check" => Token::Check,
            "return" => Token::Return,
            "true" => Token::True,
            "false" => Token::False,
            "import" => Token::Import,
            "export" => Token::Export,
            "from" => Token::From,
            "as" => Token::As,
            "library" => Token::Library,
            "extern" => Token::Extern,

            // Forbidden keywords (recognized to reject with explicit diagnostics)
            "fn" => Token::Fn,
            "mut" => Token::Mut,
            "while" => Token::While,
            "for" => Token::For,
            "struct" => Token::Struct,
            "enum" => Token::Enum,
            "dyn" => Token::Dyn,

            _ => Token::Ident(s.to_string()),
        });

    let single_token = choice((number, string, multi_sym, single_sym, ident));

    single_token
        .map_with(|tok, extra| {
            let span: SimpleSpan = extra.span();
            (tok, span)
        })
        .padded_by(comment.repeated())
        .padded()
        .repeated()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lex_basic() {
        let src = "function add(a: i32, b: i32): i32 { return a + b; }";
        let (tokens, errs) = lexer().parse(src).into_output_errors();
        assert!(errs.is_empty(), "Lex errors: {:?}", errs);
        let tokens = tokens.unwrap();
        assert_eq!(tokens[0].0, Token::Function);
    }

    #[test]
    fn test_lex_all_valid_fixtures() {
        let fixtures = [
            "valid/01_primitives.mds",
            "valid/02_functions.mds",
            "valid/03_control_flow.mds",
            "valid/04_records.mds",
            "valid/05_generics.mds",
            "valid/06_traits.mds",
            "valid/07_effects.mds",
            "valid/08_match.mds",
            "valid/09_closures.mds",
            "valid/10_arrays.mds",
            "valid/11_operators.mds",
            "valid/12_full_program.mds",
        ];

        for fixture in fixtures {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(fixture);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Could not read {}: {}", path.display(), e));
            let (tokens, errs) = lexer().parse(src.as_str()).into_output_errors();
            assert!(errs.is_empty(), "Lex errors in {}: {:?}", fixture, errs);
            assert!(tokens.is_some() && !tokens.unwrap().is_empty());
        }
    }
}
