//! Statement and block grammar parsers for Modus.

use super::common::{Span, ident_parser, to_ast_span};
use super::types::type_parser;
use crate::ast::*;
use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub fn stmt_parser_internal<'src, I, E>(
    expr: E,
) -> impl Parser<'src, I, Spanned<Stmt>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
    E: Parser<'src, I, Spanned<Expr>, extra::Err<Rich<'src, Token, Span>>> + Clone,
{
    let let_stmt = just(Token::Let)
        .ignore_then(ident_parser())
        .then(just(Token::Colon).ignore_then(type_parser()).or_not())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .then_ignore(just(Token::Semi))
        .map_with(|((name, ty), initializer), e| {
            Spanned::new(
                Stmt::Let {
                    name: name.0,
                    ty,
                    initializer,
                },
                to_ast_span(e.span()),
            )
        });

    let return_stmt = just(Token::Return)
        .ignore_then(expr.clone().or_not())
        .then_ignore(just(Token::Semi))
        .map_with(|val, e| Spanned::new(Stmt::Return(val), to_ast_span(e.span())));

    // Expression statements: If and Match don't require semicolons; others do.
    let expr_stmt = expr
        .then(just(Token::Semi).or_not())
        .try_map(|(e, semi), span| {
            if semi.is_none() && !matches!(e.node, Expr::If { .. } | Expr::Match { .. }) {
                Err(Rich::custom(
                    span,
                    "Expected ';' after expression statement",
                ))
            } else {
                Ok(Spanned::new(Stmt::Expr(e), to_ast_span(span)))
            }
        });

    choice((let_stmt, return_stmt, expr_stmt))
}

pub fn block_parser_internal<'src, I, E>(
    expr: E,
) -> impl Parser<'src, I, Vec<Spanned<Stmt>>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
    E: Parser<'src, I, Spanned<Expr>, extra::Err<Rich<'src, Token, Span>>> + Clone,
{
    let stmt = stmt_parser_internal(expr.clone());
    let trailing_expr =
        expr.map_with(|e, extra| Spanned::new(Stmt::Expr(e), to_ast_span(extra.span())));

    stmt.repeated()
        .collect::<Vec<_>>()
        .then(trailing_expr.or_not())
        .map(|(mut stmts, trailing)| {
            if let Some(t) = trailing {
                stmts.push(t);
            }
            stmts
        })
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
}

pub fn stmt_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Stmt>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    stmt_parser_internal(super::exprs::expr_parser())
}

pub fn block_parser<'src, I>()
-> impl Parser<'src, I, Vec<Spanned<Stmt>>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    block_parser_internal(super::exprs::expr_parser())
}
