//! Pattern matching grammar parser for Modus.

use super::common::{Span, ident_parser, to_ast_span};
use crate::ast::*;
use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub fn pattern_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Pattern>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    recursive(|pat| {
        let wildcard = select! {
            Token::Ident(ref s) if s == "_" => Pattern::Wildcard,
        };

        let int_lit = select! { Token::Int(n) => Literal::Int(n) };
        let float_lit = select! { Token::Float(f) => Literal::Float(f) };
        let str_lit = select! { Token::Str(s) => Literal::String(s) };
        let bool_lit = select! {
            Token::True => Literal::Bool(true),
            Token::False => Literal::Bool(false),
        };
        let neg_int = just(Token::Minus).ignore_then(select! { Token::Int(n) => Literal::Int(-n) });
        let neg_float =
            just(Token::Minus).ignore_then(select! { Token::Float(f) => Literal::Float(-f) });

        let literal = choice((neg_float, neg_int, float_lit, int_lit, str_lit, bool_lit))
            .map(Pattern::Literal);

        // Record pattern: { x: pat, y } or { x: 0, y: 0 }
        let field_pat = ident_parser()
            .then(just(Token::Colon).ignore_then(pat.clone()).or_not())
            .map(|((name, _), p)| (name, p));

        let record_pat = field_pat
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map(Pattern::Record);

        // Tuple or parenthesized pattern
        let tuple_or_paren = pat
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .map(|pats| {
                if pats.is_empty() {
                    Pattern::Literal(Literal::Unit)
                } else if pats.len() == 1 {
                    pats.into_iter().next().unwrap().node
                } else {
                    Pattern::Tuple(pats)
                }
            });

        // Variant or Ident pattern:
        // Shape.Circle(r), Shape.PointShape, Some(val), or x
        let pat_args = pat
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let variant_or_ident = ident_parser()
            .filter(|(s, _)| s != "_")
            .then(just(Token::Dot).ignore_then(ident_parser()).or_not())
            .then(pat_args.or_not())
            .map(|(((first, _), dot_second), args)| {
                if let Some((second, _)) = dot_second {
                    Pattern::Variant {
                        type_name: Some(first),
                        variant: second,
                        patterns: args.unwrap_or_default(),
                    }
                } else if let Some(args) = args {
                    Pattern::Variant {
                        type_name: None,
                        variant: first,
                        patterns: args,
                    }
                } else {
                    Pattern::Ident(first)
                }
            });

        choice((
            wildcard,
            literal,
            record_pat,
            tuple_or_paren,
            variant_or_ident,
        ))
        .map_with(|p, e| Spanned::new(p, to_ast_span(e.span())))
    })
}
