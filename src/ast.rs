use crate::diag::Span;

#[derive(Debug, Clone)]
pub struct TyExpr {
    /// Namespace qualifiers, outermost first: `git.Commit` is `["git"]`.
    pub qualifiers: Vec<String>,
    pub name: String,
    pub args: Vec<TyExpr>,
    pub span: Span,
}

/// One piece of an anyfix signature: a literal word or a bracketed parameter.
/// `ty == None` parses but the checker rejects it: parameters need a type.
#[derive(Debug, Clone)]
pub enum Part {
    Word(String),
    Param {
        name: String,
        ty: Option<TyExpr>,
    },
    /// `((name : Type, name : Type))`: one tuple parameter with a binder per component.
    Tuple(Vec<(String, TyExpr)>),
}

#[derive(Debug, Clone)]
pub struct SigAst {
    pub parts: Vec<Part>,
    pub span: Span,
}

/// One piece of a record field: a literal word or a type-only slot, which may
/// carry `DEFAULT value`.
#[derive(Debug, Clone)]
pub enum FieldPart {
    Word(String),
    Slot { ty: TyExpr, default: Option<Expr> },
}

/// A record field is an anyfix phrase: `port (Int DEFAULT 8080)`.
#[derive(Debug, Clone)]
pub struct FieldAst {
    pub parts: Vec<FieldPart>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum FnBody {
    Native(String),
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone)]
pub enum Decl {
    /// `NAMESPACE name`: file-wide; every other declaration belongs to it.
    Namespace { name: String, span: Span },
    Function {
        sig: SigAst,
        ret: Option<TyExpr>,
        body: FnBody,
        doc: Option<String>,
    },
    Record {
        name: String,
        fields: Vec<FieldAst>,
        span: Span,
        doc: Option<String>,
    },
    Enum {
        name: String,
        variants: Vec<SigAst>,
        span: Span,
        doc: Option<String>,
    },
    /// `TYPE Result (T) = Result (T) Or (String)`: `pattern` names the alias and its
    /// type parameters, `target` is what it stands for.
    Alias {
        pattern: TyExpr,
        target: TyExpr,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let(String, Expr, Span),
    Expr(Expr),
    /// `ns.{ stmts }`: inside, calls and types of `ns` resolve unqualified
    /// and win over names from outside.
    Scope(String, Vec<Stmt>, Span),
}

#[derive(Debug, Clone)]
pub enum Item {
    /// `IMPORT name`: loads the wrapper library `name` (see `library`) before
    /// anything else in the file is declared.
    Import(String, Span),
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
    /// An unresolved anyfix call (or a lone word), flattened into its words
    /// and arguments. Resolved against the signature table by the checker,
    /// which also reads a leading namespace argument (`git.status`) as a
    /// qualifier.
    Seq(Vec<SeqItem>),
    /// `[a, *b, c]`: elements may be `Spread`.
    List(Vec<Expr>),
    /// `*xs`: splices a list into the list literal that contains it.
    Spread(Box<Expr>),
    /// `(a, b)`: two or more values.
    Tuple(Vec<Expr>),
    /// `{ entry, entry }`: a record (or, later, set) literal.
    Brace(Vec<BraceEntry>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    Match(Box<Expr>, Vec<Arm>),
    /// `expr AS Type`: states the type an expression must fit.
    Ascribe(Box<Expr>, TyExpr),
    /// `(x) => body`: a function value of one parameter.
    Lambda(String, Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum SeqItem {
    Word(String, Span),
    Atom(Expr),
}

/// One anyfix phrase of a brace literal. Bare words are always part of the
/// field's name; slot values are atoms or parenthesized expressions.
#[derive(Debug, Clone)]
pub struct BraceEntry {
    pub items: Vec<SeqItem>,
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
    /// `(fixed string)` / `(foo (n))`: a slot holding a nested pattern (several
    /// items). A lone `(word)` is `Bind`; the checker turns it into a nested
    /// variant pattern when `word` names a variant of the slot's enum.
    Nested(Vec<PatItem>),
    Wild,
    /// `{ host (h), port (p) }`: destructures a record, one phrase per field.
    Record(Vec<Vec<PatItem>>),
}
