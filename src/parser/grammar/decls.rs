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
    let empty_body = just(Token::Semi).to(None);
    let concrete_body = choice((block_body, expr_body)).map(Some);
    let fn_body = choice((concrete_body, empty_body));

    just(Token::Export)
        .or_not()
        .then_ignore(just(Token::Function))
        .then(ident_parser())
        .then(fn_params)
        .then(just(Token::Colon).ignore_then(type_parser()).or_not())
        .then(fn_body)
        .map_with(
            |((((export_tok, (name, _)), (type_params, params)), return_type), body), extra| {
                Spanned::new(
                    FunctionDecl {
                        name,
                        type_params,
                        params,
                        return_type,
                        body,
                        is_exported: export_tok.is_some(),
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

    let type_decl = just(Token::Export)
        .or_not()
        .then_ignore(just(Token::Type))
        .then(ident_parser())
        .then(
            type_params
                .clone()
                .or_not()
                .map(|tp| tp.unwrap_or_default()),
        )
        .then_ignore(just(Token::Eq))
        .then(type_def)
        .then_ignore(just(Token::Semi))
        .map_with(
            |(((export_tok, (name, _)), type_params), definition), extra| {
                Spanned::new(
                    Declaration::Type(TypeDecl {
                        name,
                        type_params,
                        definition,
                        is_exported: export_tok.is_some(),
                    }),
                    to_ast_span(extra.span()),
                )
            },
        );

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

    let trait_decl = just(Token::Export)
        .or_not()
        .then_ignore(just(Token::Trait))
        .then(ident_parser())
        .then(type_params)
        .then(
            trait_member
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(((export_tok, (name, _)), type_params), members), extra| {
            Spanned::new(
                Declaration::Trait(TraitDecl {
                    name,
                    type_params,
                    members,
                    is_exported: export_tok.is_some(),
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

pub fn library_parser<'src, I>()
-> impl Parser<'src, I, Spanned<String>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let str_val = select! { Token::Str(s) => s };
    just(Token::Library)
        .ignore_then(str_val)
        .then_ignore(just(Token::Semi))
        .map_with(|s, extra| Spanned::new(s, to_ast_span(extra.span())))
}

pub fn import_parser<'src, I>()
-> impl Parser<'src, I, Spanned<ImportDecl>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let str_val = select! { Token::Str(s) => s };

    let import_specifier = ident_parser()
        .then(just(Token::As).ignore_then(ident_parser()).or_not())
        .map(|((name, _), alias)| ImportSpecifier {
            name,
            alias: alias.map(|(a, _)| a),
        });

    let named_imports = import_specifier
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map(ImportClause::Named);

    let namespace_import = just(Token::Star)
        .then_ignore(just(Token::As))
        .then(ident_parser())
        .map(|(_, (alias, _))| ImportClause::Namespace(alias));

    let clause = choice((named_imports, namespace_import)).then_ignore(just(Token::From));

    let side_effect = str_val.map(|source| ImportDecl {
        clause: ImportClause::SideEffect,
        source,
    });

    let clause_import = clause
        .then(str_val)
        .map(|(clause, source)| ImportDecl { clause, source });

    just(Token::Import)
        .ignore_then(choice((clause_import, side_effect)))
        .then_ignore(just(Token::Semi))
        .map_with(|decl, extra| Spanned::new(decl, to_ast_span(extra.span())))
}

pub fn export_clause_parser<'src, I>()
-> impl Parser<'src, I, Spanned<ExportDecl>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let str_val = select! { Token::Str(s) => s };

    let export_specifier = ident_parser()
        .then(just(Token::As).ignore_then(ident_parser()).or_not())
        .map(|((name, _), alias)| ExportSpecifier {
            name,
            alias: alias.map(|(a, _)| a),
        });

    let named_exports = export_specifier
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .then(just(Token::From).ignore_then(str_val).or_not())
        .map(|(specifiers, source)| ExportDecl::Named { specifiers, source });

    let all_export = just(Token::Star)
        .then(just(Token::As).ignore_then(ident_parser()).or_not())
        .then_ignore(just(Token::From))
        .then(str_val)
        .map(|((_, alias), source)| ExportDecl::All {
            alias: alias.map(|(a, _)| a),
            source,
        });

    just(Token::Export)
        .ignore_then(choice((named_exports, all_export)))
        .then_ignore(just(Token::Semi))
        .map_with(|decl, extra| Spanned::new(decl, to_ast_span(extra.span())))
}

#[derive(Debug, Clone, PartialEq)]
enum ProgramItem {
    Library(Spanned<String>),
    Import(Spanned<ImportDecl>),
    ExportClause(Spanned<ExportDecl>),
    Declaration(Spanned<Declaration>),
}

pub fn program_parser<'src, I>()
-> impl Parser<'src, I, Program, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    let item = choice((
        library_parser().map(ProgramItem::Library),
        import_parser().map(ProgramItem::Import),
        export_clause_parser().map(ProgramItem::ExportClause),
        decl_parser().map(ProgramItem::Declaration),
    ));

    item.repeated().collect::<Vec<_>>().map(|items| {
        let mut library = None;
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut declarations = Vec::new();

        for it in items {
            match it {
                ProgramItem::Library(lib) => {
                    if library.is_none() {
                        library = Some(lib);
                    }
                }
                ProgramItem::Import(imp) => imports.push(imp),
                ProgramItem::ExportClause(exp) => exports.push(exp),
                ProgramItem::Declaration(decl) => {
                    if decl.node.is_exported() {
                        exports.push(Spanned::new(
                            ExportDecl::Declaration(decl.clone()),
                            decl.span,
                        ));
                    }
                    declarations.push(decl);
                }
            }
        }

        Program {
            library,
            imports,
            exports,
            declarations,
        }
    })
}
