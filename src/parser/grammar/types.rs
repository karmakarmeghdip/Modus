//! Type expression grammar parser for Modus.

use super::common::{Span, ident_parser, to_ast_span};
use crate::ast::*;
use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub fn type_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Type>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    recursive(|ty| {
        let prim = select! {
            Token::Ident(ref s) if s == "u8" => PrimitiveType::U8,
            Token::Ident(ref s) if s == "u16" => PrimitiveType::U16,
            Token::Ident(ref s) if s == "u32" => PrimitiveType::U32,
            Token::Ident(ref s) if s == "u64" => PrimitiveType::U64,
            Token::Ident(ref s) if s == "i8" => PrimitiveType::I8,
            Token::Ident(ref s) if s == "i16" => PrimitiveType::I16,
            Token::Ident(ref s) if s == "i32" => PrimitiveType::I32,
            Token::Ident(ref s) if s == "i64" => PrimitiveType::I64,
            Token::Ident(ref s) if s == "f32" => PrimitiveType::F32,
            Token::Ident(ref s) if s == "f64" => PrimitiveType::F64,
            Token::Ident(ref s) if s == "bool" => PrimitiveType::Bool,
            Token::Ident(ref s) if s == "String" => PrimitiveType::String,
            Token::Ident(ref s) if s == "void" => PrimitiveType::Void,
        }
        .map(Type::Primitive)
        .map_with(|t, e| Spanned::new(t, to_ast_span(e.span())));

        // Array type [T]
        let array = ty
            .clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|inner, e| Spanned::new(Type::Array(Box::new(inner)), to_ast_span(e.span())));

        // Record type { x: T, y: U }
        let record_field = ident_parser()
            .then_ignore(just(Token::Colon))
            .then(ty.clone());

        let record = record_field
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map_with(|fields, e| {
                let fs = fields.into_iter().map(|((name, _), t)| (name, t)).collect();
                Spanned::new(Type::Record(fs), to_ast_span(e.span()))
            });

        // Path or generic type: e.g. Self.Residual or Result(T, E) or Option(T) or Drawable
        let type_args = ty
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let path_or_named = ident_parser()
            .then(
                just(Token::Dot)
                    .ignore_then(ident_parser())
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .then(type_args.or_not())
            .map_with(|((first, rest), args), e| {
                if rest.is_empty() {
                    Spanned::new(
                        Type::Generic {
                            name: first.0,
                            type_args: args.unwrap_or_default(),
                        },
                        to_ast_span(e.span()),
                    )
                } else {
                    let mut parts = vec![first.0];
                    for (part, _) in rest {
                        parts.push(part);
                    }
                    Spanned::new(Type::Path(parts), to_ast_span(e.span()))
                }
            });

        // Parenthesized types, tuples, and function types: (A, B) => C or () or (A)
        let paren_or_fn = ty
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen))
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .map_with(|(types, ret), e| {
                if let Some(ret_ty) = ret {
                    Spanned::new(
                        Type::Function {
                            param_types: types,
                            return_type: Box::new(ret_ty),
                        },
                        to_ast_span(e.span()),
                    )
                } else if types.is_empty() {
                    Spanned::new(Type::Unit, to_ast_span(e.span()))
                } else if types.len() == 1 {
                    types.into_iter().next().unwrap()
                } else {
                    Spanned::new(Type::Tuple(types), to_ast_span(e.span()))
                }
            });

        choice((prim, array, record, path_or_named, paren_or_fn))
    })
}
