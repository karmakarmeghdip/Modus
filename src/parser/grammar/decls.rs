//! Declaration and Program grammar parsers for Modus.

use super::common::{Span, ident_parser, to_ast_span};
use super::exprs::expr_parser;
use super::stmts::block_parser;
use super::types::type_parser;
use crate::ast::*;
use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

// Function declaration parser reusable by decl_parser and impl_decl
pub fn function_decl_internal<'src, I>()
-> impl Parser<'src, I, Spanned<FunctionDecl>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let type_param = ident_parser()
        .then(just(Token::Colon).ignore_then(type_parser()).or_not())
        .map(|((name, _), bound)| TypeParam { name, bound });

    let type_params = type_param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<TypeParam>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let param = ident_parser()
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .map(|((name, _), ty)| Param { name, ty });

    let params = param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<Param>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let fn_params = type_params
        .then(params.clone())
        .or(params.map(|ps| (Vec::new(), ps)));

    let block_body = block_parser().map(FunctionBody::Block);
    let expr_body = just(Token::Arrow)
        .ignore_then(expr_parser())
        .then_ignore(just(Token::Semi))
        .map(|e| FunctionBody::Expr(Box::new(e)));
    let fn_body = choice((block_body, expr_body));

    just(Token::Function)
        .ignore_then(ident_parser())
        .then(fn_params)
        .then(just(Token::Colon).ignore_then(type_parser()).or_not())
        .then(fn_body)
        .map_with(
            |((((name, _), (type_params, params)), return_type), body), extra| {
                Spanned::new(
                    FunctionDecl {
                        name,
                        type_params,
                        params,
                        return_type,
                        body,
                    },
                    to_ast_span(extra.span()),
                )
            },
        )
}

pub fn decl_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Declaration>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let type_param = ident_parser()
        .then(just(Token::Colon).ignore_then(type_parser()).or_not())
        .map(|((name, _), bound)| TypeParam { name, bound });

    let type_params = type_param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<TypeParam>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let param = ident_parser()
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .map(|((name, _), ty)| Param { name, ty });

    let params = param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<Param>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    // 1. Function declaration
    let function_decl =
        function_decl_internal().map(|f| Spanned::new(Declaration::Function(f.node), f.span));

    // 2. Type declaration: type Point = { ... }; or type Option(T) = Some(T) | None;
    let variant_fields = type_parser()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let variant = ident_parser()
        .then(variant_fields.or_not())
        .map(|((name, _), fields)| VariantDecl {
            name,
            fields: fields.unwrap_or_default(),
        });

    let union_def_multi = variant
        .clone()
        .separated_by(just(Token::Pipe))
        .at_least(2)
        .collect::<Vec<_>>()
        .map(TypeDef::Union);

    let union_def_prefixed = just(Token::Pipe)
        .ignore_then(
            variant
                .separated_by(just(Token::Pipe))
                .at_least(1)
                .collect::<Vec<_>>(),
        )
        .map(TypeDef::Union);

    let alias_def = type_parser().map(|t| TypeDef::Alias(t.node));

    let type_def = choice((union_def_prefixed, union_def_multi, alias_def))
        .map_with(|def, e| Spanned::new(def, to_ast_span(e.span())));

    let type_decl = just(Token::Type)
        .ignore_then(ident_parser())
        .then(
            type_params
                .clone()
                .or_not()
                .map(|tp| tp.unwrap_or_default()),
        )
        .then_ignore(just(Token::Eq))
        .then(type_def)
        .then_ignore(just(Token::Semi))
        .map_with(|(((name, _), type_params), definition), extra| {
            Spanned::new(
                Declaration::Type(TypeDecl {
                    name,
                    type_params,
                    definition,
                }),
                to_ast_span(extra.span()),
            )
        });

    // 3. Trait declaration: trait Drawable(Self) { function draw(self: Self): IO(void); }
    let trait_member = just(Token::Function)
        .ignore_then(ident_parser())
        .then(params)
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .then_ignore(just(Token::Semi))
        .map_with(|(((name, _), params), return_type), extra| {
            Spanned::new(
                TraitMember {
                    name,
                    params,
                    return_type,
                },
                to_ast_span(extra.span()),
            )
        });

    let trait_decl = just(Token::Trait)
        .ignore_then(ident_parser())
        .then(type_params)
        .then(
            trait_member
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(((name, _), type_params), members), extra| {
            Spanned::new(
                Declaration::Trait(TraitDecl {
                    name,
                    type_params,
                    members,
                }),
                to_ast_span(extra.span()),
            )
        });

    // 4. Impl declaration: impl Drawable for Circle { ... }
    let impl_decl = just(Token::Impl)
        .ignore_then(ident_parser())
        .then_ignore(just(Token::For))
        .then(type_parser())
        .then(
            function_decl_internal()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(((trait_name, _), target_type), methods), extra| {
            Spanned::new(
                Declaration::Impl(ImplDecl {
                    trait_name,
                    target_type,
                    methods,
                }),
                to_ast_span(extra.span()),
            )
        });

    choice((function_decl, type_decl, trait_decl, impl_decl))
}

pub fn program_parser<'src, I>()
-> impl Parser<'src, I, Program, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    decl_parser()
        .repeated()
        .collect::<Vec<_>>()
        .map(|declarations| Program { declarations })
}
