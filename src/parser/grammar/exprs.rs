//! Expression grammar parser for Modus.

use super::common::{Span, ident_parser, to_ast_span};
use super::patterns::pattern_parser;
use super::stmts::block_parser_internal;
use super::types::type_parser;
use crate::ast::*;
use crate::parser::token::Token;
use chumsky::input::ValueInput;
use chumsky::prelude::*;

pub fn expr_parser<'src, I>()
-> impl Parser<'src, I, Spanned<Expr>, extra::Err<Rich<'src, Token, Span>>> + Clone
where
    I: ValueInput<'src, Token = Token, Span = Span>,
{
    recursive(|expr| {
        let block = block_parser_internal(expr.clone()).boxed();

        // Literals
        let int_lit = select! { Token::Int(n) => Literal::Int(n) }
            .map_with(|lit, extra| Spanned::new(Expr::Literal(lit), to_ast_span(extra.span())));

        let float_lit = select! { Token::Float(f) => Literal::Float(f) }
            .map_with(|lit, extra| Spanned::new(Expr::Literal(lit), to_ast_span(extra.span())));

        let str_lit = select! { Token::Str(s) => Literal::String(s) }
            .map_with(|lit, extra| Spanned::new(Expr::Literal(lit), to_ast_span(extra.span())));

        let bool_lit = select! {
            Token::True => Literal::Bool(true),
            Token::False => Literal::Bool(false),
        }
        .map_with(|lit, extra| Spanned::new(Expr::Literal(lit), to_ast_span(extra.span())));

        let unit_lit = just(Token::LParen)
            .then_ignore(just(Token::RParen))
            .map_with(|_, extra| {
                Spanned::new(Expr::Literal(Literal::Unit), to_ast_span(extra.span()))
            });

        // Identifiers
        let ident_expr = ident_parser()
            .map_with(|(s, _), extra| Spanned::new(Expr::Ident(s), to_ast_span(extra.span())));

        // Array literal [a, b, c]
        let array_literal = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|elements, extra| {
                Spanned::new(Expr::Array(elements), to_ast_span(extra.span()))
            });

        // Record update: { ...base, field: expr, ... }
        let record_update_field = ident_parser()
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map(|((name, _), val)| (name, val));

        let record_update = just(Token::Spread)
            .ignore_then(expr.clone())
            .then(
                just(Token::Comma)
                    .ignore_then(record_update_field)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .then_ignore(just(Token::Comma).or_not())
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map_with(|(base, fields), extra| {
                Spanned::new(
                    Expr::RecordUpdate {
                        base: Box::new(base),
                        fields,
                    },
                    to_ast_span(extra.span()),
                )
            });

        // Record literal: { field: expr, ... } (at least 1 field)
        let record_field = ident_parser()
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map(|((name, _), val)| (name, val));

        let record_literal = record_field
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .at_least(1)
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map_with(|fields, extra| {
                Spanned::new(Expr::Record(fields), to_ast_span(extra.span()))
            });

        // Closure expression: (x: i32, y: i32): Ret => expr / block
        let closure_param = ident_parser()
            .then_ignore(just(Token::Colon))
            .then(type_parser())
            .map(|((name, _), ty)| Param { name, ty });

        let closure_params = closure_param
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<Param>>()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let closure_body = choice((
            block.clone().map(FunctionBody::Block),
            expr.clone().map(|e| FunctionBody::Expr(Box::new(e))),
        ));

        let closure_expr = closure_params
            .then(just(Token::Colon).ignore_then(type_parser()).or_not())
            .then_ignore(just(Token::Arrow))
            .then(closure_body)
            .map_with(|((params, return_type), body), extra| {
                Spanned::new(
                    Expr::Closure {
                        params,
                        return_type,
                        body,
                    },
                    to_ast_span(extra.span()),
                )
            });

        // If expression: if (cond) { ... } else if ... else { ... }
        let else_branch = just(Token::Else).ignore_then(choice((
            expr.clone()
                .filter(|e: &Spanned<Expr>| matches!(e.node, Expr::If { .. }))
                .map(|e| ElseBranch::If(Box::new(e))),
            block.clone().map(ElseBranch::Block),
        )));

        let if_expr = just(Token::If)
            .ignore_then(
                expr.clone()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
            .then(block.clone())
            .then(else_branch.or_not())
            .map_with(|((condition, then_branch), else_b), extra| {
                Spanned::new(
                    Expr::If {
                        condition: Box::new(condition),
                        then_branch,
                        else_branch: else_b,
                    },
                    to_ast_span(extra.span()),
                )
            });

        // Match expression: match expr { pat => body, ... }
        let arm = pattern_parser()
            .then_ignore(just(Token::Arrow))
            .then(choice((
                block.clone().map(MatchArmBody::Block),
                expr.clone().map(MatchArmBody::Expr),
            )))
            .map(|(pattern, body)| MatchArm { pattern, body });

        let match_expr = just(Token::Match)
            .ignore_then(expr.clone())
            .then(
                arm.separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map_with(|(scrutinee, arms), extra| {
                Spanned::new(
                    Expr::Match {
                        expr: Box::new(scrutinee),
                        arms,
                    },
                    to_ast_span(extra.span()),
                )
            });

        // Block expression: { stmts }
        let block_expr = block
            .clone()
            .map_with(|stmts, extra| Spanned::new(Expr::Block(stmts), to_ast_span(extra.span())));

        // Parenthesized expression (expr)
        let paren_expr = expr
            .clone()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let atom = choice((
            float_lit,
            int_lit,
            str_lit,
            bool_lit,
            closure_expr,
            unit_lit,
            paren_expr,
            ident_expr,
            array_literal,
            record_update,
            record_literal,
            if_expr,
            match_expr,
            block_expr,
        ))
        .boxed();

        // Postfix operations: calls, method calls, field access, index
        enum PostfixAction {
            MethodCall(String, Vec<Spanned<Expr>>, crate::ast::Span),
            FieldAccess(String, crate::ast::Span),
            Index(Spanned<Expr>, crate::ast::Span),
            Call(Vec<Spanned<Expr>>, crate::ast::Span),
        }

        let args = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        let dot_call_or_field = just(Token::Dot)
            .ignore_then(ident_parser())
            .then(args.clone().or_not())
            .map_with(|((name, _), args_opt), extra| {
                let span = to_ast_span(extra.span());
                if let Some(args) = args_opt {
                    PostfixAction::MethodCall(name, args, span)
                } else {
                    PostfixAction::FieldAccess(name, span)
                }
            });

        let index_action = expr
            .clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|idx, extra| PostfixAction::Index(idx, to_ast_span(extra.span())));

        let call_action =
            args.map_with(|args, extra| PostfixAction::Call(args, to_ast_span(extra.span())));

        let postfix_action = choice((dot_call_or_field, index_action, call_action));

        let postfix = atom
            .foldl(postfix_action.repeated(), |receiver, action| match action {
                PostfixAction::MethodCall(method, args, action_span) => {
                    let span = crate::ast::Span::new(receiver.span.start, action_span.end);
                    Spanned::new(
                        Expr::MethodCall {
                            receiver: Box::new(receiver),
                            method,
                            args,
                        },
                        span,
                    )
                }
                PostfixAction::FieldAccess(field, action_span) => {
                    let span = crate::ast::Span::new(receiver.span.start, action_span.end);
                    Spanned::new(
                        Expr::FieldAccess {
                            receiver: Box::new(receiver),
                            field,
                        },
                        span,
                    )
                }
                PostfixAction::Index(index, action_span) => {
                    let span = crate::ast::Span::new(receiver.span.start, action_span.end);
                    Spanned::new(
                        Expr::Index {
                            receiver: Box::new(receiver),
                            index: Box::new(index),
                        },
                        span,
                    )
                }
                PostfixAction::Call(args, action_span) => {
                    let span = crate::ast::Span::new(receiver.span.start, action_span.end);
                    Spanned::new(
                        Expr::Call {
                            callee: Box::new(receiver),
                            args,
                        },
                        span,
                    )
                }
            })
            .boxed();

        // Prefix unary operators: !, -, perform, check
        let prefix_op = choice((
            just(Token::Bang).map_with(|_, e| (UnaryOp::Not, to_ast_span(e.span()))),
            just(Token::Minus).map_with(|_, e| (UnaryOp::Neg, to_ast_span(e.span()))),
            just(Token::Perform).map_with(|_, e| (UnaryOp::Perform, to_ast_span(e.span()))),
            just(Token::Check).map_with(|_, e| (UnaryOp::Check, to_ast_span(e.span()))),
        ));

        let unary = prefix_op
            .repeated()
            .collect::<Vec<_>>()
            .then(postfix)
            .map(|(ops, expr)| {
                ops.into_iter().rfold(expr, |acc, (op, op_span)| {
                    let span = crate::ast::Span::new(op_span.start, acc.span.end);
                    Spanned::new(
                        Expr::Unary {
                            op,
                            expr: Box::new(acc),
                        },
                        span,
                    )
                })
            })
            .boxed();

        // Binary operators with precedence climbing
        // Level 1: Multiplicative (*, /, %)
        let mul_op = choice((
            just(Token::Star).to(BinaryOp::Mul),
            just(Token::Slash).to(BinaryOp::Div),
            just(Token::Percent).to(BinaryOp::Rem),
        ));
        let mul = unary
            .clone()
            .foldl(mul_op.then(unary).repeated(), |lhs, (op, rhs)| {
                let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                Spanned::new(
                    Expr::Binary {
                        lhs: Box::new(lhs),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                )
            })
            .boxed();

        // Level 2: Additive (+, -)
        let add_op = choice((
            just(Token::Plus).to(BinaryOp::Add),
            just(Token::Minus).to(BinaryOp::Sub),
        ));
        let add = mul
            .clone()
            .foldl(add_op.then(mul).repeated(), |lhs, (op, rhs)| {
                let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                Spanned::new(
                    Expr::Binary {
                        lhs: Box::new(lhs),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                )
            })
            .boxed();

        // Level 3: Comparison (<, <=, >, >=)
        let cmp_op = choice((
            just(Token::LtEq).to(BinaryOp::LtEq),
            just(Token::GtEq).to(BinaryOp::GtEq),
            just(Token::Lt).to(BinaryOp::Lt),
            just(Token::Gt).to(BinaryOp::Gt),
        ));
        let cmp = add
            .clone()
            .foldl(cmp_op.then(add).repeated(), |lhs, (op, rhs)| {
                let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                Spanned::new(
                    Expr::Binary {
                        lhs: Box::new(lhs),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                )
            })
            .boxed();

        // Level 4: Equality (==, !=)
        let eq_op = choice((
            just(Token::EqEq).to(BinaryOp::Eq),
            just(Token::BangEq).to(BinaryOp::NotEq),
        ));
        let eq = cmp
            .clone()
            .foldl(eq_op.then(cmp).repeated(), |lhs, (op, rhs)| {
                let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                Spanned::new(
                    Expr::Binary {
                        lhs: Box::new(lhs),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                )
            })
            .boxed();

        // Level 5: Logical AND (&&)
        let and = eq
            .clone()
            .foldl(
                just(Token::AndAnd).to(BinaryOp::And).then(eq).repeated(),
                |lhs, (op, rhs)| {
                    let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                    Spanned::new(
                        Expr::Binary {
                            lhs: Box::new(lhs),
                            op,
                            rhs: Box::new(rhs),
                        },
                        span,
                    )
                },
            )
            .boxed();

        // Level 6: Logical OR (||)
        and.clone().foldl(
            just(Token::OrOr).to(BinaryOp::Or).then(and).repeated(),
            |lhs, (op, rhs)| {
                let span = crate::ast::Span::new(lhs.span.start, rhs.span.end);
                Spanned::new(
                    Expr::Binary {
                        lhs: Box::new(lhs),
                        op,
                        rhs: Box::new(rhs),
                    },
                    span,
                )
            },
        )
    })
}
