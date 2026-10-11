use crate::lexer::Span;

#[derive(Debug, Clone)]
pub struct TyExpr {
    pub name: String,
    pub args: Vec<TyExpr>,
    pub span: Span,
}

/// One piece of a mixfix signature: a literal word or a bracketed parameter.
/// `ty == None` is only legal for `(self)` inside a RECORD.
#[derive(Debug, Clone)]
pub enum Part {
    Word(String),
    Param { name: String, ty: Option<TyExpr> },
}

#[derive(Debug, Clone)]
pub struct SigAst {
    pub parts: Vec<Part>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FieldAst {
    pub sig: SigAst,
    pub optional: bool,
    pub ty: TyExpr,
}

#[derive(Debug, Clone)]
pub enum FnBody {
    Native(String),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone)]
pub enum Decl {
    Function {
        sig: SigAst,
        ret: Option<TyExpr>,
        body: FnBody,
    },
    Record {
        name: String,
        fields: Vec<FieldAst>,
        span: Span,
    },
    Enum {
        name: String,
        variants: Vec<SigAst>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(String, Expr, Span),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub enum Item {
    Decl(Decl),
    Stmt(Stmt),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub k: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(String),
    Placeholder,
    /// An unresolved mixfix call (or a lone word). Resolved against the
    /// signature table by the checker.
    Seq(Vec<SeqItem>),
    List(Vec<Expr>),
    Brace(Vec<BraceField>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    Pipe(Box<Expr>, Box<Expr>),
    /// `expr : Type` — states the type an expression must fit.
    Ascribe(Box<Expr>, TyExpr),
}

#[derive(Debug, Clone)]
pub enum SeqItem {
    Word(String, Span),
    Atom(Expr),
}

#[derive(Debug, Clone)]
pub struct BraceField {
    pub key: Vec<String>,
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pat: Vec<PatItem>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PatItem {
    Word(String),
    Bind(String),
    Wild,
}
