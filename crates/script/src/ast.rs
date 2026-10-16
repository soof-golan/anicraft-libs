//! The syntax tree of the supported subset. Exporters compile it; the interpreter walks it.

use serde::{Deserialize, Serialize};

/// A parsed script: its statements and what could not be understood.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Script {
    pub stmts: Vec<Stmt>,
    /// Constructs outside the subset, one per statement that was skipped.
    pub diagnostics: Vec<Diagnostic>,
}

/// A problem found while parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// 1-based line.
    pub line: u32,
    pub message: String,
}

/// A function: a declaration, or the literal of a handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Func {
    pub name: Option<String>,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

/// A statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Expr(Expr),
    Var {
        name: String,
        init: Option<Expr>,
    },
    If {
        cond: Expr,
        then: Vec<Self>,
        otherwise: Vec<Self>,
    },
    Block(Vec<Self>),
    Function(Func),
    Return(Option<Expr>),
    /// A statement outside the subset. It does nothing when run; the parser also reports a [`Diagnostic`].
    Unsupported {
        line: u32,
        text: String,
    },
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    Not,
    Neg,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// An expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    This,
    Ident(String),
    Member(Box<Self>, String),
    Call(Box<Self>, Vec<Self>),
    Unary(UnOp, Box<Self>),
    Binary(BinOp, Box<Self>, Box<Self>),
    /// `target = value`, or `target += value` when the operator is given.
    Assign(Box<Self>, Option<BinOp>, Box<Self>),
    /// `target++` / `--target` and friends: the amount added, and whether the new value is the result.
    Update {
        target: Box<Self>,
        delta: f64,
        prefix: bool,
    },
    Function(Box<Func>),
}

impl Expr {
    /// The dotted path of a chain of names (`a.b.c`), or `None` for anything else.
    #[must_use]
    pub fn path(&self) -> Option<Vec<&str>> {
        match self {
            Self::Ident(n) => Some(vec![n.as_str()]),
            Self::This => Some(vec!["this"]),
            Self::Member(base, name) => {
                let mut p = base.path()?;
                p.push(name.as_str());
                Some(p)
            }
            _ => None,
        }
    }
}
