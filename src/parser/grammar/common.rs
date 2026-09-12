//! Common parser types and utilities.

use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub type Span = SimpleSpan;

pub fn to_ast_span(s: Span) -> crate::ast::Span {
    crate::ast::Span::new(s.start, s.end)
}

pub fn ident_parser<'src, I>()
-> impl Parser<'src, I, (String, Span), extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! {
        Token::Ident(s) => s,
    }
    .map_with(|s, extra| (s, extra.span()))
}

pub fn str_parser<'src, I>()
-> impl Parser<'src, I, (String, Span), extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    select! {
        Token::Str(s) => s,
    }
    .map_with(|s, extra| (s, extra.span()))
}
