//! Modus Tokens for lexical analysis.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Keywords
    Function,
    Type,
    Let,
    If,
    Else,
    Match,
    Trait,
    Impl,
    Perform,
    Check,
    Return,
    True,
    False,
    Import,
    Export,
    From,
    As,
    Library,
    Extern,

    // Forbidden keywords (recognized to reject with explicit diagnostics)
    Fn,
    Mut,
    While,
    For,
    Struct,
    Enum,
    Dyn,

    // Identifiers & Literals
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    TemplateStr(String),

    // Multi-character Symbols & Operators
    Arrow,  // =>
    Spread, // ...
    EqEq,   // ==
    BangEq, // !=
    LtEq,   // <=
    GtEq,   // >=
    AndAnd, // &&
    OrOr,   // ||

    // Single-character Symbols & Operators
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    Percent,  // %
    Lt,       // <
    Gt,       // >
    Bang,     // !
    Eq,       // =
    Pipe,     // |
    Colon,    // :
    Semi,     // ;
    Comma,    // ,
    Dot,      // .
    LParen,   // (
    RParen,   // )
    LBrace,   // {
    RBrace,   // }
    LBracket, // [
    RBracket, // ]
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Function => write!(f, "'function'"),
            Token::Type => write!(f, "'type'"),
            Token::Let => write!(f, "'let'"),
            Token::If => write!(f, "'if'"),
            Token::Else => write!(f, "'else'"),
            Token::Match => write!(f, "'match'"),
            Token::Trait => write!(f, "'trait'"),
            Token::Impl => write!(f, "'impl'"),
            Token::Perform => write!(f, "'perform'"),
            Token::Check => write!(f, "'check'"),
            Token::Return => write!(f, "'return'"),
            Token::True => write!(f, "'true'"),
            Token::False => write!(f, "'false'"),
            Token::Import => write!(f, "'import'"),
            Token::Export => write!(f, "'export'"),
            Token::From => write!(f, "'from'"),
            Token::As => write!(f, "'as'"),
            Token::Library => write!(f, "'library'"),
            Token::Extern => write!(f, "'extern'"),
            Token::Fn => write!(f, "'fn'"),
            Token::Mut => write!(f, "'mut'"),
            Token::While => write!(f, "'while'"),
            Token::For => write!(f, "'for'"),
            Token::Struct => write!(f, "'struct'"),
            Token::Enum => write!(f, "'enum'"),
            Token::Dyn => write!(f, "'dyn'"),
            Token::Ident(s) => write!(f, "identifier '{}'", s),
            Token::Int(i) => write!(f, "integer '{}'", i),
            Token::Float(fl) => write!(f, "float '{}'", fl),
            Token::Str(s) => write!(f, "string \"{}\"", s),
            Token::TemplateStr(s) => write!(f, "template string `{}`", s),
            Token::Arrow => write!(f, "'=>'"),
            Token::Spread => write!(f, "'...'"),
            Token::EqEq => write!(f, "'=='"),
            Token::BangEq => write!(f, "'!='"),
            Token::LtEq => write!(f, "'<='"),
            Token::GtEq => write!(f, "'>='"),
            Token::AndAnd => write!(f, "'&&'"),
            Token::OrOr => write!(f, "'||'"),
            Token::Plus => write!(f, "'+'"),
            Token::Minus => write!(f, "'-'"),
            Token::Star => write!(f, "'*'"),
            Token::Slash => write!(f, "'/'"),
            Token::Percent => write!(f, "'%'"),
            Token::Lt => write!(f, "'<'"),
            Token::Gt => write!(f, "'>'"),
            Token::Bang => write!(f, "'!'"),
            Token::Eq => write!(f, "'='"),
            Token::Pipe => write!(f, "'|'"),
            Token::Colon => write!(f, "':'"),
            Token::Semi => write!(f, "';'"),
            Token::Comma => write!(f, "','"),
            Token::Dot => write!(f, "'.'"),
            Token::LParen => write!(f, "'('"),
            Token::RParen => write!(f, "')'"),
            Token::LBrace => write!(f, "'{{'"),
            Token::RBrace => write!(f, "'}}'"),
            Token::LBracket => write!(f, "'['"),
            Token::RBracket => write!(f, "']'"),
        }
    }
}
