//! Declaration table, overload resolution and type checking.
//!
//! Type checking is bottom-up and one-way: the type of an expression depends
//! only on its children, never on where it is used. Parsing leaves every call
//! as a flat `Seq` of words/atoms. Because every argument is exactly one
//! token, a call can only match a signature with the same length and the same
//! words in the word positions. Several signatures may share those words and
//! differ in parameter types (overloading); the arguments are inferred once
//! and the overload is chosen by testing `argument type <: parameter type`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast::*;
use crate::eval::{NATIVES, Value};
use crate::lexer::{Diag, Res, Span};
use crate::pattern::{Pat, Sig, enum_params, render, useful, witnesses};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    List(Box<Type>),
    /// A set of variants of an enum; at run time a list of variants.
    Set(Box<Type>),
    /// A function value of one parameter: `Function (A) To (B)`.
    Function(Box<Type>, Box<Type>),
    /// Two or more values of their own types: `(a, b)`.
    Tuple(Vec<Type>),
    /// The inferred type of a `(x) => body` literal. Its parameter type is
    /// only known from a sibling argument, so it is checked against the
    /// `Function` parameter of each candidate overload.
    Lambda,
    Record(String),
    Enum(String, Vec<Type>),
    Param(String),
    /// The bottom type: a subtype of everything. The element type of `[]`,
    /// the payload of `none`, and any generic parameter nothing constrains.
    Never,
    /// The inferred type of a `{ ... }` literal: one entry per field phrase,
    /// as its signature key (`port _`, `size between _ and _`) and the types of
    /// its slot values. It only becomes a `Record` when it meets a record
    /// parameter or a `: Record Name` annotation.
    Struct(Vec<(String, Vec<Type>)>),
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Float => write!(f, "Float"),
            Type::Bool => write!(f, "Boolean"),
            Type::Str => write!(f, "String"),
            Type::Unit => write!(f, "Unit"),
            Type::List(t) => write!(f, "List ({t})"),
            Type::Set(t) => write!(f, "Set ({t})"),
            Type::Function(a, r) => write!(f, "Function ({a}) To ({r})"),
            Type::Tuple(ts) => {
                let ts: Vec<String> = ts.iter().map(Type::to_string).collect();
                write!(f, "({})", ts.join(", "))
            }
            Type::Lambda => write!(f, "lambda"),
            Type::Record(n) => write!(f, "{n}"),
            Type::Enum(n, args) if args.is_empty() => write!(f, "{n}"),
            Type::Enum(n, args) if n == "Result" && args.len() == 2 && args[1] == Type::Str => {
                write!(f, "Result ({})", args[0])
            }
            Type::Enum(n, args) if n == "Result" && args.len() == 2 => {
                write!(f, "Result ({}) Or ({})", args[0], args[1])
            }
            Type::Enum(n, args) => {
                write!(f, "{n}")?;
                args.iter().try_for_each(|t| write!(f, " ({t})"))
            }
            Type::Param(p) => write!(f, "{p}"),
            Type::Never => write!(f, "Never"),
            Type::Struct(fs) if fs.is_empty() => write!(f, "record literal {{}}"),
            Type::Struct(fs) => {
                write!(f, "record literal {{ ")?;
                for (i, (k, ts)) in fs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", signature(k, ts))?;
                }
                write!(f, " }}")
            }
        }
    }
}

/// `a <: b`: every value of type `a` is a valid `b`. Covariant everywhere
/// (values are immutable). Record literals are handled by `Checker::fits`.
pub fn sub(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Never, _) => true,
        (Type::List(x), Type::List(y)) | (Type::Set(x), Type::Set(y)) => sub(x, y),
        (Type::Function(p, r), Type::Function(q, s)) => sub(q, p) && sub(r, s),
        (Type::Tuple(xs), Type::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| sub(x, y))
        }
        (Type::Enum(n, xs), Type::Enum(m, ys)) => {
            n == m && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| sub(x, y))
        }
        _ => a == b,
    }
}

/// The smaller of two comparable types (the one both fit into), if any.
fn join(a: &Type, b: &Type) -> Option<Type> {
    if sub(a, b) {
        Some(b.clone())
    } else if sub(b, a) {
        Some(a.clone())
    } else {
        None
    }
}

/// Could one argument type match both declared parameter types? Used to
/// reject overloads that would be ambiguous at some call.
fn overlap(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Param(_), _) | (_, Type::Param(_)) => true,
        (Type::List(x), Type::List(y)) | (Type::Set(x), Type::Set(y)) => overlap(x, y),
        (Type::Function(p, r), Type::Function(q, s)) => overlap(p, q) && overlap(r, s),
        (Type::Tuple(xs), Type::Tuple(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| overlap(x, y))
        }
        (Type::Enum(n, xs), Type::Enum(m, ys)) => {
            n == m && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| overlap(x, y))
        }
        _ => a == b,
    }
}

/// Substitutes bound parameters; `keep_unbound` leaves unbound ones as-is
/// (for messages), otherwise they become `Never`.
pub(crate) fn subst(t: &Type, m: &HashMap<String, Type>, keep_unbound: bool) -> Type {
    match t {
        Type::Param(n) => match m.get(n) {
            Some(x) => x.clone(),
            None if keep_unbound => t.clone(),
            None => Type::Never,
        },
        Type::List(x) => Type::List(Box::new(subst(x, m, keep_unbound))),
        Type::Set(x) => Type::Set(Box::new(subst(x, m, keep_unbound))),
        Type::Function(a, r) => Type::Function(
            Box::new(subst(a, m, keep_unbound)),
            Box::new(subst(r, m, keep_unbound)),
        ),
        Type::Tuple(xs) => Type::Tuple(xs.iter().map(|x| subst(x, m, keep_unbound)).collect()),
        Type::Enum(n, a) => Type::Enum(
            n.clone(),
            a.iter().map(|x| subst(x, m, keep_unbound)).collect(),
        ),
        other => other.clone(),
    }
}

/// Does `t` mention a generic parameter that `m` has not bound yet?
fn unbound_param(t: &Type, m: &HashMap<String, Type>) -> bool {
    match t {
        Type::Param(n) => !m.contains_key(n),
        Type::List(x) | Type::Set(x) => unbound_param(x, m),
        Type::Function(a, r) => unbound_param(a, m) || unbound_param(r, m),
        Type::Enum(_, xs) | Type::Tuple(xs) => xs.iter().any(|x| unbound_param(x, m)),
        _ => false,
    }
}

/// A field key with each `_` filled by the next slot type: `port (Int)`.
fn signature(key: &str, slots: &[Type]) -> String {
    let mut tys = slots.iter();
    key.split(' ')
        .map(|w| {
            if w != "_" {
                w.to_string()
            } else {
                tys.next().map_or("(?)".to_string(), |t| format!("({t})"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone)]
pub enum PartT {
    Word(String),
    Param(String, Type),
    /// A tuple parameter: a binder and type per component.
    Tuple(Vec<(String, Type)>),
}

impl PartT {
    /// The parameter this part declares, if it is one: its name and type. A
    /// tuple parameter is named by its components, which no word can spell.
    pub fn slot(&self) -> Option<(String, Type)> {
        match self {
            PartT::Word(_) => None,
            PartT::Param(n, t) => Some((n.clone(), t.clone())),
            PartT::Tuple(cs) => Some((
                format!(
                    "({})",
                    cs.iter()
                        .map(|(n, _)| n.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                Type::Tuple(cs.iter().map(|(_, t)| t.clone()).collect()),
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FnKind {
    User {
        params: Vec<String>,
        body: Option<Ir>,
    },
    Native(String),
    Variant {
        enum_name: String,
        idx: usize,
    },
}

#[derive(Debug, Clone)]
pub struct FnDef {
    /// The namespace this function was declared in (`None` for the prelude
    /// and for files without a `NAMESPACE`).
    pub ns: Option<String>,
    pub parts: Vec<PartT>,
    pub ret: Type,
    pub kind: FnKind,
    /// The `//` comment lines directly above the declaration, if any.
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VariantDef {
    pub parts: Vec<PartT>,
    pub fn_id: usize,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub params: Vec<String>,
    pub variants: Vec<VariantDef>,
    /// The `//` comment lines directly above the declaration, if any.
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SlotDef {
    pub ty: Type,
    /// The `DEFAULT` value, checked and ready to evaluate.
    pub default: Option<Ir>,
}

/// A record field: an anyfix phrase with type-only slots.
#[derive(Debug, Clone)]
pub struct FieldDef {
    /// The words with `_` for each slot: `size between _ and _`.
    pub key: String,
    pub slots: Vec<SlotDef>,
}

impl FieldDef {
    pub fn slot_types(&self) -> Vec<Type> {
        self.slots.iter().map(|s| s.ty.clone()).collect()
    }

    /// The field as written in a signature: `port (Int)`.
    pub fn signature(&self) -> String {
        signature(&self.key, &self.slot_types())
    }

    /// The key a bare Boolean field name takes in a literal: `{ verbose }`
    /// means `{ verbose true }`. Only a field whose single slot is a Boolean.
    fn bare_key(&self) -> Option<String> {
        matches!(self.slots.as_slice(), [SlotDef { ty: Type::Bool, .. }]).then(|| {
            self.key
                .split(' ')
                .filter(|w| *w != "_")
                .collect::<Vec<_>>()
                .join(" ")
        })
    }
}

#[derive(Debug, Clone)]
pub struct RecordDef {
    pub fields: Vec<FieldDef>,
    /// The `//` comment lines directly above the declaration, if any.
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IrArm {
    pub pat: Pat,
    pub body: Ir,
}

#[derive(Debug, Clone)]
pub enum Ir {
    Lit(Value),
    Var(String),
    Call(usize, Vec<Ir>),
    Let(String, Box<Ir>),
    Block(Vec<Ir>),
    List(Vec<Ir>),
    /// Record type name; every slot value in declaration order.
    MkRecord(String, Vec<Ir>),
    /// A `{ ... }` literal not yet fitted to a record; never reaches `eval`.
    /// Each entry is its signature key and its slot values.
    Struct(Vec<(String, Vec<Ir>)>),
    If(Box<Ir>, Box<Ir>, Box<Ir>),
    Match(Box<Ir>, Vec<IrArm>),
    Tuple(Vec<Ir>),
    /// `...xs` inside a list literal.
    Spread(Box<Ir>),
    /// Binds the components of the tuple held by a variable, in the body.
    Untuple(String, Vec<String>, Box<Ir>),
    Pipe(Box<Ir>, Box<Ir>),
    /// A function value: parameter name and body.
    Lambda(String, Box<Ir>),
}

pub type Scope = Vec<(String, Type)>;

#[derive(Debug, Clone)]
pub struct Checker {
    pub defs: Vec<FnDef>,
    pub enums: HashMap<String, EnumDef>,
    pub records: HashMap<String, RecordDef>,
    /// Type aliases by qualified pattern name (`Result _`): parameter names and target.
    aliases: HashMap<String, (Vec<String>, TyExpr)>,
    /// The file being checked: its declarations are added to this namespace,
    /// and its unqualified calls and types resolve in it (plus the prelude).
    cur_ns: Option<String>,
    namespaces: HashSet<String>,
    /// Namespaces opened by enclosing `ns:: { ... }` blocks, outermost first.
    open: RefCell<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Record,
    Enum,
    Function,
}

/// A record field or an enum variant.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// `port (Int)` for a field, `file` or `link (target: String)` for a variant.
    pub signature: String,
    /// Per slot of a record field, its `DEFAULT` as text when it is a literal
    /// or a plain variant (`None` when it has none or is not renderable).
    /// Empty for variants.
    pub default: Vec<Option<String>>,
}

/// One entry of `Checker::symbols`.
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub kind: SymbolKind,
    /// Record: `List Options`. Enum with parameters: `Option (T)`.
    /// Function: its signature, `find (root: String) with (options: fd::Options)`.
    pub name: String,
    pub doc: Option<String>,
    /// Record fields or enum variants; empty for functions.
    pub members: Vec<Member>,
    /// The return type of a function (`List (Entry)`); `None` for types.
    pub ret: Option<String>,
}

/// Removes every `prefix` (`ns::`) that starts a qualified name, leaving
/// longer identifiers that merely end in the same letters (`xns::T`) alone.
fn strip_qualifier(text: &str, prefix: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(prefix) {
        let before = rest[..i].chars().next_back().or(out.chars().next_back());
        out.push_str(&rest[..i]);
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == ':') {
            out.push_str(prefix);
        }
        rest = &rest[i + prefix.len()..];
    }
    out.push_str(rest);
    out
}

pub fn label(parts: &[PartT]) -> String {
    parts
        .iter()
        .map(|p| match p {
            PartT::Word(w) => w.clone(),
            PartT::Param(n, t) => format!("({n}: {t})"),
            PartT::Tuple(cs) => format!(
                "(({}))",
                cs.iter()
                    .map(|(n, t)| format!("{n}: {t}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn seq_text(items: &[SeqItem]) -> String {
    items
        .iter()
        .map(|i| match i {
            SeqItem::Word(w, _) => w.clone(),
            SeqItem::Atom(_) => "<arg>".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The call as written, with literal arguments shown (`"a" 3 (f x)`), for
/// diagnostics that must let the reader see exactly what was parsed.
fn seq_source(items: &[SeqItem]) -> String {
    items
        .iter()
        .map(|i| match i {
            SeqItem::Word(w, _) => w.clone(),
            SeqItem::Atom(e) => expr_source(e),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn expr_source(e: &Expr) -> String {
    match &e.k {
        ExprKind::Str(s) => format!("{s:?}"),
        ExprKind::Int(n) => n.to_string(),
        ExprKind::Float(f) => f.to_string(),
        ExprKind::Placeholder => "_".to_string(),
        ExprKind::Seq(items) => match items.as_slice() {
            [SeqItem::Word(w, _)] => w.clone(),
            _ => format!("({})", seq_source(items)),
        },
        ExprKind::Qualified(q, inner) => format!("{q}::{}", expr_source(inner)),
        ExprKind::List(_) => "[...]".to_string(),
        ExprKind::Brace(_) => "{...}".to_string(),
        _ => "(...)".to_string(),
    }
}

fn count_placeholders(e: &Expr) -> usize {
    match &e.k {
        ExprKind::Placeholder => 1,
        ExprKind::Lambda(_, body) => count_placeholders(body),
        ExprKind::Seq(items) => items
            .iter()
            .map(|i| match i {
                SeqItem::Atom(e) => count_placeholders(e),
                _ => 0,
            })
            .sum(),
        ExprKind::List(xs) | ExprKind::Tuple(xs) => xs.iter().map(count_placeholders).sum(),
        ExprKind::Spread(e) => count_placeholders(e),
        ExprKind::Brace(fs) => fs
            .iter()
            .flat_map(|f| &f.items)
            .map(|i| match i {
                SeqItem::Atom(e) => count_placeholders(e),
                SeqItem::Word(..) => 0,
            })
            .sum(),
        ExprKind::If(a, b, c) => {
            count_placeholders(a) + count_placeholders(b) + count_placeholders(c)
        }
        ExprKind::Match(s, arms) => {
            count_placeholders(s)
                + arms
                    .iter()
                    .map(|a| count_placeholders(&a.body))
                    .sum::<usize>()
        }
        ExprKind::Ascribe(e, _) | ExprKind::Qualified(_, e) => count_placeholders(e),
        // The right side of a nested pipe binds its own `_`.
        ExprKind::Pipe(l, _) => count_placeholders(l),
        _ => 0,
    }
}

fn nested_namespace(chain: &str, span: Span) -> Diag {
    Diag::new(
        span,
        format!(
            "namespaces are not nested: a file has one `NAMESPACE name`, so `{chain}` cannot be resolved; write a single qualifier, e.g. `name::call`"
        ),
    )
}

impl Default for Checker {
    fn default() -> Self {
        Checker::new()
    }
}

impl Checker {
    pub fn new() -> Checker {
        let mut ck = Checker {
            defs: vec![],
            enums: HashMap::new(),
            records: HashMap::new(),
            aliases: HashMap::new(),
            cur_ns: None,
            namespaces: HashSet::new(),
            open: RefCell::new(Vec::new()),
        };
        let t = Type::Param("T".into());
        let e = Type::Param("E".into());
        let option = Type::Enum("Option".into(), vec![t.clone()]);
        let result = Type::Enum("Result".into(), vec![t.clone(), e.clone()]);
        let w = |s: &str| PartT::Word(s.to_string());
        let add_enum =
            |ck: &mut Checker, name: &str, params: &[&str], vs: Vec<Vec<PartT>>, ret: Type| {
                let mut variants = vec![];
                for (idx, parts) in vs.into_iter().enumerate() {
                    let id = ck.defs.len();
                    ck.defs.push(FnDef {
                        ns: None,
                        parts: parts.clone(),
                        ret: ret.clone(),
                        kind: FnKind::Variant {
                            enum_name: name.to_string(),
                            idx,
                        },
                        doc: None,
                    });
                    variants.push(VariantDef { parts, fn_id: id });
                }
                ck.enums.insert(
                    name.to_string(),
                    EnumDef {
                        params: params.iter().map(|s| s.to_string()).collect(),
                        variants,
                        doc: None,
                    },
                );
            };
        add_enum(
            &mut ck,
            "Option",
            &["T"],
            vec![
                vec![w("some"), PartT::Param("value".into(), t.clone())],
                vec![w("none")],
            ],
            option,
        );
        add_enum(
            &mut ck,
            "Result",
            &["T", "E"],
            vec![
                vec![w("ok"), PartT::Param("value".into(), t)],
                vec![w("error"), PartT::Param("error".into(), e)],
            ],
            result,
        );
        ck
    }

    // ---------- declarations ----------

    fn resolve_ty(&self, t: &TyExpr) -> Res<Type> {
        let qualified = match t.qualifiers.as_slice() {
            [] => None,
            [q] => {
                self.known_namespace(q, t.span)?;
                Some(format!("{q}::{}", t.name))
            }
            qs => {
                return Err(nested_namespace(
                    &format!("{}::{}", qs.join("::"), t.name),
                    t.span,
                ));
            }
        };
        if let Some(full) = qualified {
            return self
                .named_ty(&full)
                .ok_or_else(|| Diag::new(t.span, format!("unknown type `{full}`")));
        }
        let args: Res<Vec<Type>> = t.args.iter().map(|a| self.resolve_ty(a)).collect();
        let args = args?;
        let arity = |n: usize, ty: Type| -> Res<Type> {
            if args.len() == n {
                Ok(ty)
            } else {
                Err(Diag::new(
                    t.span,
                    format!("type `{}` takes {n} type argument(s)", t.name),
                ))
            }
        };
        match t.name.as_str() {
            "Int" => arity(0, Type::Int),
            "Float" => arity(0, Type::Float),
            "Boolean" => arity(0, Type::Bool),
            "String" => arity(0, Type::Str),
            "Unit" => arity(0, Type::Unit),
            "List _" => arity(
                1,
                Type::List(Box::new(args.first().cloned().unwrap_or(Type::Never))),
            ),
            "Function _ To _" => arity(
                2,
                Type::Function(Box::new(args[0].clone()), Box::new(args[1].clone())),
            ),
            "Set _" => match args.first() {
                Some(Type::Enum(..) | Type::Param(_) | Type::Never) | None => arity(
                    1,
                    Type::Set(Box::new(args.first().cloned().unwrap_or(Type::Never))),
                ),
                Some(other) => Err(Diag::new(
                    t.span,
                    format!("`Set` holds the variants of an enum, found {other}"),
                )),
            },
            "Option _" => arity(1, Type::Enum("Option".into(), args.clone())),
            "Result _ Or _" => arity(2, Type::Enum("Result".into(), args.clone())),
            n if n.len() == 1 && n.chars().all(|c| c.is_ascii_uppercase()) => {
                Ok(Type::Param(n.to_string()))
            }
            n if self.find_alias(n).is_some() => {
                let (params, target) = self.find_alias(n).unwrap();
                arity(params.len(), Type::Never)?;
                let body = self.resolve_ty(&target)?;
                let map: HashMap<String, Type> = params.into_iter().zip(args).collect();
                Ok(subst(&body, &map, true))
            }
            n => match self
                .cur_ns
                .as_ref()
                .into_iter()
                .chain(self.open.borrow().iter())
                .rev()
                .find_map(|ns| self.named_ty(&format!("{ns}::{n}")))
                .or_else(|| self.named_ty(n))
            {
                Some(ty) => arity(0, ty),
                None => Err(Diag::new(t.span, format!("unknown type `{n}`"))),
            },
        }
    }

    /// Makes `ns` the namespace of the file about to be checked.
    pub fn set_namespace(&mut self, ns: Option<String>) {
        if let Some(ns) = &ns {
            self.namespaces.insert(ns.clone());
        }
        self.cur_ns = ns;
    }

    /// The table key of a type declared in the current file.
    fn qualify(&self, name: &str) -> String {
        match &self.cur_ns {
            Some(ns) => format!("{ns}::{name}"),
            None => name.to_string(),
        }
    }

    /// The alias called `name` (`Result _`), looked up like a named type.
    fn find_alias(&self, name: &str) -> Option<(Vec<String>, TyExpr)> {
        self.cur_ns
            .as_ref()
            .into_iter()
            .chain(self.open.borrow().iter())
            .rev()
            .find_map(|ns| self.aliases.get(&format!("{ns}::{name}")))
            .or_else(|| self.aliases.get(name))
            .cloned()
    }

    fn named_ty(&self, key: &str) -> Option<Type> {
        if self.records.contains_key(key) {
            Some(Type::Record(key.to_string()))
        } else if self.enums.contains_key(key) {
            Some(Type::Enum(key.to_string(), vec![]))
        } else {
            None
        }
    }

    fn unknown_namespace_message(&self, q: &str) -> String {
        let mut known: Vec<&str> = self.namespaces.iter().map(String::as_str).collect();
        known.sort();
        format!(
            "unknown namespace `{q}`; declared namespaces: {}",
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        )
    }

    /// Declared namespaces, sorted.
    pub fn namespace_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.namespaces.iter().cloned().collect();
        names.sort();
        names
    }

    fn known_namespace(&self, q: &str, span: Span) -> Res<()> {
        if self.namespaces.contains(q) {
            return Ok(());
        }
        Err(Diag::new(span, self.unknown_namespace_message(q)))
    }

    /// Every symbol declared directly in a namespace (`None` is the global one:
    /// the prelude, built-ins and files without a `NAMESPACE`). Records and
    /// enums come first, sorted by name, then functions in declaration order.
    pub fn symbols(&self, ns: Option<&str>) -> Result<Vec<Symbol>, String> {
        if let Some(q) = ns
            && !self.namespaces.contains(q)
        {
            return Err(self.unknown_namespace_message(q));
        }
        let prefix = ns.map(|n| format!("{n}::")).unwrap_or_default();
        // A key belongs to the namespace when it is `ns::Name` (or has no `::`
        // for the global namespace).
        let local = |key: &str| -> Option<String> {
            match ns {
                Some(_) => key.strip_prefix(&prefix).map(str::to_string),
                None => (!key.contains("::")).then(|| key.to_string()),
            }
        };
        let mut types: Vec<Symbol> = Vec::new();
        for (key, r) in &self.records {
            let Some(name) = local(key) else { continue };
            let members = r
                .fields
                .iter()
                .map(|f| Member {
                    signature: f.signature(),
                    default: f
                        .slots
                        .iter()
                        .map(|s| s.default.as_ref().and_then(|d| self.default_text(d)))
                        .collect(),
                })
                .collect();
            types.push(Symbol {
                kind: SymbolKind::Record,
                name,
                doc: r.doc.clone(),
                members,
                ret: None,
            });
        }
        for (key, e) in &self.enums {
            let Some(base) = local(key) else { continue };
            let name = match (base.as_str(), e.params.as_slice()) {
                ("Result", [t, u]) => format!("Result ({t}) Or ({u})"),
                (_, ps) => ps
                    .iter()
                    .fold(base.clone(), |acc, p| format!("{acc} ({p})")),
            };
            types.push(Symbol {
                kind: SymbolKind::Enum,
                name,
                doc: e.doc.clone(),
                members: e
                    .variants
                    .iter()
                    .map(|v| Member {
                        signature: label(&v.parts),
                        default: Vec::new(),
                    })
                    .collect(),
                ret: None,
            });
        }
        types.sort_by(|a, b| a.name.cmp(&b.name));
        let fns = self
            .defs
            .iter()
            .filter(|d| d.ns.as_deref() == ns && !matches!(d.kind, FnKind::Variant { .. }))
            .map(|d| Symbol {
                kind: SymbolKind::Function,
                name: label(&d.parts),
                doc: d.doc.clone(),
                members: Vec::new(),
                ret: Some(d.ret.to_string()),
            });
        types.extend(fns);
        // Inside `ns`, references to its own types need no `ns::` qualifier.
        if ns.is_some() {
            for s in &mut types {
                s.name = strip_qualifier(&s.name, &prefix);
                s.ret = s.ret.as_deref().map(|r| strip_qualifier(r, &prefix));
                for m in &mut s.members {
                    m.signature = strip_qualifier(&m.signature, &prefix);
                }
            }
        }
        Ok(types)
    }

    /// A field default as source text: a literal, or a variant without arguments.
    fn default_text(&self, ir: &Ir) -> Option<String> {
        match ir {
            Ir::Lit(Value::Int(n)) => Some(n.to_string()),
            Ir::Lit(Value::Float(x)) => Some(x.to_string()),
            Ir::Lit(Value::Bool(b)) => Some(b.to_string()),
            Ir::Lit(Value::Str(s)) => Some(format!("{s:?}")),
            Ir::Call(id, args) if args.is_empty() => match &self.defs.get(*id)?.kind {
                FnKind::Variant { .. } => Some(label(&self.defs[*id].parts)),
                _ => None,
            },
            _ => None,
        }
    }

    fn mk_parts(&self, sig: &SigAst) -> Res<Vec<PartT>> {
        let mut out = Vec::new();
        for p in &sig.parts {
            match p {
                Part::Word(w) => out.push(PartT::Word(w.clone())),
                Part::Param { name, ty: Some(t) } => {
                    out.push(PartT::Param(name.clone(), self.resolve_ty(t)?))
                }
                Part::Tuple(comps) => {
                    let mut cs = Vec::new();
                    for (name, t) in comps {
                        cs.push((name.clone(), self.resolve_ty(t)?));
                    }
                    out.push(PartT::Tuple(cs));
                }
                Part::Param { name, ty: None } => {
                    return Err(Diag::new(
                        sig.span,
                        format!("parameter `{name}` needs a type, e.g. `({name}: Path)`"),
                    ));
                }
            }
        }
        if !out.iter().any(|p| matches!(p, PartT::Word(_))) {
            return Err(Diag::new(
                sig.span,
                "a signature needs at least one literal word",
            ));
        }
        Ok(out)
    }

    fn add_def(&mut self, parts: Vec<PartT>, ret: Type, kind: FnKind, span: Span) -> Res<usize> {
        for d in &self.defs {
            // Same words in the same positions, and every parameter could
            // receive the same argument type: a call could not choose.
            let same = d.ns == self.cur_ns
                && d.parts.len() == parts.len()
                && d.parts.iter().zip(&parts).all(|(a, b)| match (a, b) {
                    (PartT::Word(x), PartT::Word(y)) => x == y,
                    (PartT::Word(_), _) | (_, PartT::Word(_)) => false,
                    (a, b) => match (a.slot(), b.slot()) {
                        (Some((_, x)), Some((_, y))) => overlap(&x, &y),
                        _ => false,
                    },
                });
            if same {
                return Err(Diag::new(
                    span,
                    format!(
                        "signature `{}` clashes with existing `{}`: same words in the same positions and no parameter type tells them apart. Overloads must differ in at least one parameter by type constructor (e.g. `Int` vs `String`, `List (T)` vs `Set (T)`); `List (T)` vs `List (Int)` is specialization and is not allowed",
                        label(&parts),
                        label(&d.parts)
                    ),
                ));
            }
        }
        self.defs.push(FnDef {
            ns: self.cur_ns.clone(),
            parts,
            ret,
            kind,
            doc: None,
        });
        Ok(self.defs.len() - 1)
    }

    fn type_name_free(&self, name: &str, span: Span) -> Res<()> {
        if self.records.contains_key(name) || self.enums.contains_key(name) {
            return Err(Diag::new(span, format!("type `{name}` is already defined")));
        }
        Ok(())
    }

    /// Registers declarations. Returns function bodies that still need checking.
    pub fn declare<'a>(&mut self, decls: &[&'a Decl]) -> Res<Vec<(usize, &'a FnBody, Span)>> {
        for d in decls {
            match d {
                Decl::Record {
                    name, span, doc, ..
                } => {
                    let key = self.qualify(name);
                    self.type_name_free(&key, *span)?;
                    self.records.insert(
                        key,
                        RecordDef {
                            fields: vec![],
                            doc: doc.clone(),
                        },
                    );
                }
                Decl::Enum {
                    name, span, doc, ..
                } => {
                    let key = self.qualify(name);
                    self.type_name_free(&key, *span)?;
                    self.enums.insert(
                        key,
                        EnumDef {
                            params: vec![],
                            variants: vec![],
                            doc: doc.clone(),
                        },
                    );
                }
                Decl::Alias {
                    pattern,
                    target,
                    span,
                } => {
                    let mut params = Vec::new();
                    for a in &pattern.args {
                        let single =
                            a.name.len() == 1 && a.name.chars().all(|c| c.is_ascii_uppercase());
                        if !single || !a.args.is_empty() || !a.qualifiers.is_empty() {
                            return Err(Diag::new(
                                a.span,
                                "an alias parameter is a single capital letter, e.g. `TYPE Result (T) = ...`",
                            ));
                        }
                        params.push(a.name.clone());
                    }
                    let key = self.qualify(&pattern.name);
                    if self.aliases.contains_key(&key) {
                        return Err(Diag::new(
                            *span,
                            format!("alias `{}` is already defined", pattern.name),
                        ));
                    }
                    self.aliases.insert(key, (params, target.clone()));
                }
                _ => {}
            }
        }
        let mut pending = Vec::new();
        for d in decls {
            match d {
                Decl::Record { name, fields, .. } => {
                    let key = self.qualify(name);
                    let mut fds: Vec<FieldDef> = Vec::new();
                    for f in fields {
                        let fd = self.mk_field(f)?;
                        self.check_field_clash(&key, &fds, &fd, f.span)?;
                        fds.push(fd);
                    }
                    self.records.get_mut(&key).unwrap().fields = fds;
                }
                Decl::Enum { name, variants, .. } => {
                    let key = self.qualify(name);
                    let ety = Type::Enum(key.clone(), vec![]);
                    let mut vds = Vec::new();
                    for v in variants {
                        let parts = self.mk_parts(v)?;
                        if parts.iter().any(|p| matches!(p, PartT::Tuple(_))) {
                            return Err(Diag::new(
                                v.span,
                                "an enum variant cannot take a tuple slot; give it one slot per value",
                            ));
                        }
                        let idx = vds.len();
                        let fid = self.add_def(
                            parts.clone(),
                            ety.clone(),
                            FnKind::Variant {
                                enum_name: key.clone(),
                                idx,
                            },
                            v.span,
                        )?;
                        vds.push(VariantDef { parts, fn_id: fid });
                    }
                    self.enums.get_mut(&key).unwrap().variants = vds;
                }
                Decl::Namespace { .. } | Decl::Alias { .. } => {}
                Decl::Function {
                    sig,
                    ret,
                    body,
                    doc,
                } => {
                    let parts = self.mk_parts(sig)?;
                    let rt = match ret {
                        Some(t) => self.resolve_ty(t)?,
                        None => Type::Unit,
                    };
                    let params = parts
                        .iter()
                        .filter_map(|p| p.slot().map(|(n, _)| n))
                        .collect();
                    let kind = match body {
                        FnBody::Native(n) => {
                            if !NATIVES.contains(&n.as_str()) {
                                return Err(Diag::new(
                                    sig.span,
                                    format!("unknown native function `{n}`"),
                                ));
                            }
                            FnKind::Native(n.clone())
                        }
                        FnBody::Block(_) => FnKind::User { params, body: None },
                    };
                    let id = self.add_def(parts, rt, kind, sig.span)?;
                    self.defs[id].doc = doc.clone();
                    if let FnBody::Block(_) = body {
                        pending.push((id, body, sig.span));
                    }
                }
            }
        }
        self.check_defaults(decls)?;
        Ok(pending)
    }

    /// The field of a `RECORD` declaration, with its slot types resolved.
    fn mk_field(&self, f: &FieldAst) -> Res<FieldDef> {
        let mut key = Vec::new();
        let mut slots = Vec::new();
        for part in &f.parts {
            match part {
                FieldPart::Word(w) => key.push(w.as_str()),
                FieldPart::Slot { ty, .. } => {
                    key.push("_");
                    slots.push(SlotDef {
                        ty: self.resolve_ty(ty)?,
                        default: None,
                    });
                }
            }
        }
        Ok(FieldDef {
            key: key.join(" "),
            slots,
        })
    }

    /// A literal entry must name exactly one field, so two fields may not
    /// share a key, nor may a bare Boolean field name collide with another key.
    fn check_field_clash(
        &self,
        rn: &str,
        before: &[FieldDef],
        fd: &FieldDef,
        span: Span,
    ) -> Res<()> {
        for other in before {
            let same = other.key == fd.key
                || other.bare_key().is_some_and(|b| b == fd.key)
                || fd.bare_key().is_some_and(|b| b == other.key);
            if same {
                return Err(Diag::new(
                    span,
                    format!(
                        "field `{}` of `{rn}` clashes with field `{}`: a literal entry could not tell them apart (a bare Boolean field name stands for `name true`)",
                        fd.signature(),
                        other.signature()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Checks every `DEFAULT value` once all declarations are registered, so a
    /// default may call any function of the file.
    fn check_defaults(&mut self, decls: &[&Decl]) -> Res<()> {
        for d in decls {
            let Decl::Record { name, fields, .. } = d else {
                continue;
            };
            let key = self.qualify(name);
            let mut checked: Vec<(usize, usize, Ir)> = Vec::new();
            for (fi, f) in fields.iter().enumerate() {
                let slot_asts = f.parts.iter().filter_map(|p| match p {
                    FieldPart::Slot { default, .. } => Some(default),
                    FieldPart::Word(_) => None,
                });
                for (si, default) in slot_asts.enumerate() {
                    let Some(e) = default else { continue };
                    let want = self.records[&key].fields[fi].slots[si].ty.clone();
                    let (ir, t) = self.infer(e, &mut Vec::new())?;
                    if !self.fits(&t, &want) {
                        return Err(Diag::new(
                            e.span,
                            self.mismatch(
                                &format!(
                                    "DEFAULT of `{}` in `{key}`",
                                    self.records[&key].fields[fi].signature()
                                ),
                                &want,
                                &t,
                            ),
                        ));
                    }
                    checked.push((fi, si, self.fit(ir, &t, &want)));
                }
            }
            let rd = self.records.get_mut(&key).unwrap();
            for (fi, si, ir) in checked {
                rd.fields[fi].slots[si].default = Some(ir);
            }
        }
        Ok(())
    }

    pub fn check_bodies(&mut self, pending: Vec<(usize, &FnBody, Span)>) -> Res<()> {
        for (id, body, span) in pending {
            let ir = self.check_fn_body(id, body, span)?;
            if let FnKind::User { body, .. } = &mut self.defs[id].kind {
                *body = Some(ir);
            }
        }
        Ok(())
    }

    fn check_fn_body(&self, id: usize, body: &FnBody, span: Span) -> Res<Ir> {
        let def = &self.defs[id];
        let mut sc: Scope = Vec::new();
        let mut tuples: Vec<(String, Vec<String>)> = Vec::new();
        for p in &def.parts {
            match p {
                PartT::Word(_) => {}
                PartT::Param(n, t) => sc.push((n.clone(), t.clone())),
                PartT::Tuple(cs) => {
                    sc.extend(cs.iter().cloned());
                    let names = cs.iter().map(|(n, _)| n.clone()).collect();
                    tuples.push((p.slot().expect("a tuple is a parameter").0, names));
                }
            }
        }
        let FnBody::Block(stmts) = body else {
            unreachable!()
        };
        let (mut ir, t) = self.block(stmts, &mut sc)?;
        for (var, names) in tuples.into_iter().rev() {
            ir = Ir::Untuple(var, names, Box::new(ir));
        }
        if !sub(&t, &def.ret) {
            return Err(Diag::new(
                span,
                format!(
                    "function `{}` is declared to return {} but its body has type {}",
                    label(&def.parts),
                    def.ret,
                    t
                ),
            ));
        }
        Ok(ir)
    }

    // ---------- record literals ----------

    /// Does a value of type `a` fit where `b` is expected? Subtyping, plus
    /// record literals fitting a record whose fields they cover.
    pub fn fits(&self, a: &Type, b: &Type) -> bool {
        match (a, b) {
            (Type::Struct(fs), Type::Record(rn)) => self.struct_problem(fs, rn).is_none(),
            (Type::Struct(fs), Type::Set(e)) => self.set_problem(fs, e).is_none(),
            (Type::Struct(_), _) => false,
            _ => sub(a, b),
        }
    }

    /// The field a literal entry names: an exact key match, or a bare Boolean
    /// field name (no slot values) standing for `name true`. The flag says
    /// whether the entry was the bare form.
    fn field_for(rd: &RecordDef, key: &str, slot_values: usize) -> Option<(usize, bool)> {
        rd.fields
            .iter()
            .position(|d| d.key == key)
            .map(|i| (i, false))
            .or_else(|| {
                (slot_values == 0)
                    .then(|| {
                        rd.fields
                            .iter()
                            .position(|d| d.bare_key().as_deref() == Some(key))
                    })
                    .flatten()
                    .map(|i| (i, true))
            })
    }

    /// The variant of enum type `e` a literal entry names, if any.
    fn variant_for(&self, e: &Type, key: &str, slot_values: usize) -> Option<(usize, usize)> {
        let Type::Enum(en, _) = e else { return None };
        self.enums[en.as_str()]
            .variants
            .iter()
            .enumerate()
            .find(|(_, v)| {
                let k = v
                    .parts
                    .iter()
                    .map(|p| match p {
                        PartT::Word(w) => w.as_str(),
                        PartT::Param(..) | PartT::Tuple(_) => "_",
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                k == key
                    && v.parts
                        .iter()
                        .filter(|p| matches!(p, PartT::Param(..)))
                        .count()
                        == slot_values
            })
            .map(|(i, v)| (i, v.fn_id))
    }

    /// Why a `{ ... }` literal with these entries is not a `Set (e)`.
    fn set_problem(&self, fs: &[(String, Vec<Type>)], e: &Type) -> Option<String> {
        let Type::Enum(en, _) = e else {
            return Some(format!("`Set ({e})` does not hold the variants of an enum"));
        };
        for (k, ts) in fs {
            let Some((_, fid)) = self.variant_for(e, k, ts.len()) else {
                let known: Vec<String> = self.enums[en.as_str()]
                    .variants
                    .iter()
                    .map(|v| label(&v.parts))
                    .collect();
                return Some(format!(
                    "enum `{en}` has no variant `{}`; its variants are: {}",
                    signature(k, ts),
                    known.join(", ")
                ));
            };
            let FnKind::Variant { .. } = self.defs[fid].kind else {
                continue;
            };
            let slots = self.defs[fid].parts.iter().filter_map(|p| match p {
                PartT::Param(_, t) => Some(t),
                PartT::Word(_) | PartT::Tuple(_) => None,
            });
            for (want, t) in slots.zip(ts) {
                if !self.fits(t, want) {
                    return Some(format!(
                        "variant `{}` of `{en}` expects {want}, found {t}",
                        label(&self.defs[fid].parts)
                    ));
                }
            }
        }
        None
    }

    /// Why a record literal with these entries does not fit record `rn`.
    fn struct_problem(&self, fs: &[(String, Vec<Type>)], rn: &str) -> Option<String> {
        let rd = &self.records[rn];
        let mut given = vec![false; rd.fields.len()];
        for (k, ts) in fs {
            let Some((fi, bare)) = Self::field_for(rd, k, ts.len()) else {
                let known: Vec<String> = rd.fields.iter().map(FieldDef::signature).collect();
                return Some(format!(
                    "record `{rn}` has no field `{}`; its fields are: {}",
                    signature(k, ts),
                    if known.is_empty() {
                        "none".to_string()
                    } else {
                        known.join(", ")
                    }
                ));
            };
            let d = &rd.fields[fi];
            if std::mem::replace(&mut given[fi], true) {
                return Some(format!(
                    "field `{}` of `{rn}` is given twice",
                    d.signature()
                ));
            }
            if bare {
                continue;
            }
            for (want, t) in d.slots.iter().zip(ts) {
                if !self.fits(t, &want.ty) {
                    return Some(format!(
                        "field `{}` of `{rn}` expects {}, found {t}",
                        d.signature(),
                        want.ty
                    ));
                }
            }
        }
        for (d, g) in rd.fields.iter().zip(&given) {
            if !g && d.slots.iter().any(|s| s.default.is_none()) {
                return Some(format!(
                    "record `{rn}` is missing required field `{}` (only fields whose slots all have a DEFAULT may be omitted)",
                    d.signature()
                ));
            }
        }
        None
    }

    /// Turns an expression of type `ty` that `fits` `target` into one whose
    /// runtime shape is `target`'s: record literals become real records, with
    /// omitted slots filled from their `DEFAULT`.
    fn fit(&self, ir: Ir, ty: &Type, target: &Type) -> Ir {
        if let (Ir::Struct(given), Type::Struct(tys), Type::Set(e)) = (&ir, ty, target) {
            let irs = given
                .iter()
                .zip(tys)
                .map(|((k, irs), (_, ts))| {
                    let (_, fid) = self
                        .variant_for(e, k, irs.len())
                        .expect("checked by `fits`");
                    let slots = self.defs[fid].parts.iter().filter_map(|p| match p {
                        PartT::Param(_, t) => Some(t),
                        PartT::Word(_) | PartT::Tuple(_) => None,
                    });
                    let args = irs
                        .iter()
                        .zip(ts)
                        .zip(slots)
                        .map(|((ir, t), want)| self.fit(ir.clone(), t, want))
                        .collect();
                    Ir::Call(fid, args)
                })
                .collect();
            return Ir::List(irs);
        }
        let (Ir::Struct(given), Type::Struct(tys), Type::Record(rn)) = (&ir, ty, target) else {
            return ir;
        };
        let rd = &self.records[rn];
        let mut values: Vec<Vec<Ir>> = vec![Vec::new(); rd.fields.len()];
        for ((k, irs), (_, ts)) in given.iter().zip(tys) {
            let (fi, bare) = Self::field_for(rd, k, irs.len()).expect("checked by `fits`");
            let slots = &rd.fields[fi].slots;
            values[fi] = if bare {
                vec![Ir::Lit(Value::Bool(true))]
            } else {
                irs.iter()
                    .zip(ts)
                    .zip(slots)
                    .map(|((ir, t), slot)| self.fit(ir.clone(), t, &slot.ty))
                    .collect()
            };
        }
        let irs = rd
            .fields
            .iter()
            .zip(values)
            .flat_map(|(d, given)| {
                if given.is_empty() {
                    d.slots
                        .iter()
                        .map(|s| s.default.clone().expect("checked by `fits`"))
                        .collect()
                } else {
                    given
                }
            })
            .collect();
        Ir::MkRecord(rn.clone(), irs)
    }

    fn reject_struct(t: &Type, span: Span) -> Res<()> {
        if matches!(t, Type::Lambda) {
            return Err(Diag::new(
                span,
                "a lambda has no type of its own; pass it where a function is expected, after the argument that fixes its parameter type",
            ));
        }
        if matches!(t, Type::Struct(_)) {
            return Err(Diag::new(
                span,
                format!(
                    "a record literal has no type of its own here (it is {t}); annotate it: `{{ ... }} : Record Name`, or pass it where a record is expected"
                ),
            ));
        }
        Ok(())
    }

    // ---------- expressions ----------

    pub fn block(&self, stmts: &[Stmt], sc: &mut Scope) -> Res<(Ir, Type)> {
        let mark = sc.len();
        let mut irs = Vec::new();
        let mut last = Type::Unit;
        for s in stmts {
            match s {
                Stmt::Let(n, e, _) => {
                    let (ir, t) = self.check(e, sc)?;
                    sc.push((n.clone(), t));
                    irs.push(Ir::Let(n.clone(), Box::new(ir)));
                    last = Type::Unit;
                }
                Stmt::Expr(e) => {
                    let (ir, t) = self.check(e, sc)?;
                    irs.push(ir);
                    last = t;
                }
                Stmt::Discard(e) => {
                    let (ir, _) = self.check(e, sc)?;
                    irs.push(ir);
                    irs.push(Ir::Lit(Value::Unit));
                    last = Type::Unit;
                }
                Stmt::Scope(ns, inner, span) => {
                    self.open_scope(ns, *span)?;
                    let checked = self.block(inner, sc);
                    self.close_scope();
                    let (ir, t) = checked?;
                    irs.push(ir);
                    last = t;
                }
            }
        }
        sc.truncate(mark);
        Ok((Ir::Block(irs), last))
    }

    fn check_as(&self, e: &Expr, sc: &mut Scope, ty: &Type) -> Res<Ir> {
        let (ir, t) = self.check(e, sc)?;
        if sub(&t, ty) {
            Ok(ir)
        } else {
            Err(Diag::new(e.span, format!("expected {ty}, found {t}")))
        }
    }

    /// Infers the type of `e`, bottom-up. A bare record literal is rejected
    /// here; only arguments, record fields and `:` annotations may take one.
    pub fn check(&self, e: &Expr, sc: &mut Scope) -> Res<(Ir, Type)> {
        let (ir, t) = self.infer(e, sc)?;
        Self::reject_struct(&t, e.span)?;
        Ok((ir, t))
    }

    fn infer(&self, e: &Expr, sc: &mut Scope) -> Res<(Ir, Type)> {
        match &e.k {
            ExprKind::Int(n) => Ok((Ir::Lit(Value::Int(*n)), Type::Int)),
            ExprKind::Float(f) => Ok((Ir::Lit(Value::Float(*f)), Type::Float)),
            ExprKind::Str(s) => Ok((Ir::Lit(Value::Str(s.clone())), Type::Str)),
            ExprKind::Placeholder => match sc.iter().rev().find(|(n, _)| n == "_") {
                Some((_, t)) => Ok((Ir::Var("_".into()), t.clone())),
                None => Err(Diag::new(
                    e.span,
                    "`_` can only be used on the right side of `|>`",
                )),
            },
            ExprKind::Seq(items) => self.seq(items, e.span, sc, None),
            ExprKind::Qualified(q, inner) => {
                self.known_namespace(q, e.span)?;
                match &inner.k {
                    ExprKind::Seq(items) => self.seq(items, inner.span, sc, Some(q)),
                    ExprKind::Qualified(q2, _) => {
                        Err(nested_namespace(&format!("{q}::{q2}::..."), e.span))
                    }
                    _ => Err(Diag::new(
                        inner.span,
                        format!("`{q}::` must be followed by a call, not a literal"),
                    )),
                }
            }
            ExprKind::List(xs) => {
                let mut ty = Type::Never;
                let mut irs = Vec::new();
                for x in xs {
                    let (ir, t) = match &x.k {
                        ExprKind::Spread(inner) => {
                            let (ir, t) = self.check(inner, sc)?;
                            let Type::List(elem) = t else {
                                return Err(Diag::new(
                                    inner.span,
                                    format!("`...` splices a list into a list literal, found {t}"),
                                ));
                            };
                            irs.push(Ir::Spread(Box::new(ir)));
                            let joined = join(&ty, &elem).ok_or_else(|| {
                                Diag::new(
                                    x.span,
                                    format!(
                                        "list elements must share one type: earlier elements are {ty}, the spread list holds {elem}"
                                    ),
                                )
                            })?;
                            ty = joined;
                            continue;
                        }
                        _ => self.check(x, sc)?,
                    };
                    ty = join(&ty, &t).ok_or_else(|| {
                        Diag::new(
                            x.span,
                            format!(
                                "list elements must share one type: earlier elements are {ty}, this one is {t}"
                            ),
                        )
                    })?;
                    irs.push(ir);
                }
                Ok((Ir::List(irs), Type::List(Box::new(ty))))
            }
            ExprKind::Spread(_) => Err(Diag::new(
                e.span,
                "`...` only works inside a list literal, e.g. `[...xs, 1]`",
            )),
            ExprKind::Tuple(xs) => {
                let mut irs = Vec::new();
                let mut tys = Vec::new();
                for x in xs {
                    let (ir, t) = self.check(x, sc)?;
                    irs.push(ir);
                    tys.push(t);
                }
                Ok((Ir::Tuple(irs), Type::Tuple(tys)))
            }
            ExprKind::Brace(fs) => self.brace(fs, sc),
            ExprKind::Lambda(..) => Ok((Ir::Lit(Value::Unit), Type::Lambda)),
            ExprKind::If(c, t, f) => {
                let cir = self.check_as(c, sc, &Type::Bool)?;
                let (tir, tt) = self.check(t, sc)?;
                let (fir, ft) = self.check(f, sc)?;
                let Some(ty) = join(&tt, &ft) else {
                    return Err(Diag::new(
                        f.span,
                        format!("IF branches must have the same type: THEN is {tt}, ELSE is {ft}"),
                    ));
                };
                Ok((Ir::If(Box::new(cir), Box::new(tir), Box::new(fir)), ty))
            }
            ExprKind::Match(s, arms) => self.matching(s, arms, e.span, sc),
            ExprKind::Pipe(l, r) => {
                let n = count_placeholders(r);
                if n != 1 {
                    return Err(Diag::new(
                        r.span,
                        format!(
                            "the right side of `|>` must contain exactly one `_` placeholder, found {n}"
                        ),
                    ));
                }
                let (lir, lt) = self.check(l, sc)?;
                let mark = sc.len();
                sc.push(("_".into(), lt));
                let res = self.check(r, sc);
                sc.truncate(mark);
                let (rir, rt) = res?;
                Ok((Ir::Pipe(Box::new(lir), Box::new(rir)), rt))
            }
            ExprKind::Ascribe(inner, ty) => {
                let target = self.resolve_ty(ty)?;
                let (ir, t) = self.infer(inner, sc)?;
                if !self.fits(&t, &target) {
                    return Err(Diag::new(
                        inner.span,
                        self.mismatch(&format!("annotation `: {target}`"), &target, &t),
                    ));
                }
                Ok((self.fit(ir, &t, &target), target))
            }
        }
    }

    /// "expected X, found Y", plus the reason when Y is a record literal.
    fn mismatch(&self, what: &str, want: &Type, found: &Type) -> String {
        let mut msg = format!("{what} expects {want}, found {found}");
        if let (Type::Struct(fs), Type::Record(rn)) = (found, want)
            && let Some(why) = self.struct_problem(fs, rn)
        {
            msg.push_str(&format!(": {why}"));
        }
        if let (Type::Struct(fs), Type::Set(e)) = (found, want)
            && let Some(why) = self.set_problem(fs, e)
        {
            msg.push_str(&format!(": {why}"));
        }
        msg
    }

    /// A record literal is inferred from its entries alone. Bare words of an
    /// entry belong to the field name (except `true`/`false`); slot values are
    /// atoms or parenthesized expressions, each inferred on its own.
    fn brace(&self, fs: &[BraceEntry], sc: &mut Scope) -> Res<(Ir, Type)> {
        let mut irs: Vec<(String, Vec<Ir>)> = Vec::new();
        let mut tys: Vec<(String, Vec<Type>)> = Vec::new();
        for f in fs {
            let mut key = Vec::new();
            let mut eirs = Vec::new();
            let mut etys = Vec::new();
            for item in &f.items {
                match item {
                    // `true` and `false` are the Boolean literals, not name words:
                    // `{ verbose true }`.
                    SeqItem::Word(w, _) if w == "true" || w == "false" => {
                        key.push("_");
                        eirs.push(Ir::Lit(Value::Bool(w == "true")));
                        etys.push(Type::Bool);
                    }
                    SeqItem::Word(w, _) => key.push(w.as_str()),
                    SeqItem::Atom(e) => {
                        key.push("_");
                        let (ir, t) = self.infer(e, sc)?;
                        eirs.push(ir);
                        etys.push(t);
                    }
                }
            }
            let key = key.join(" ");
            if let Some(first) = tys.iter().position(|(k, _)| k == &key) {
                let at = fs[first].span;
                return Err(Diag::new(
                    f.span,
                    format!(
                        "field `{}` is given twice: first at {}:{}, again at {}:{}",
                        signature(&key, &etys),
                        at.line,
                        at.col,
                        f.span.line,
                        f.span.col
                    ),
                ));
            }
            irs.push((key.clone(), eirs));
            tys.push((key, etys));
        }
        Ok((Ir::Struct(irs), Type::Struct(tys)))
    }

    /// `{ field (name), ... }` against record `rn`: every field listed once, in
    /// any order. The pattern is one constructor over the record's slots in
    /// declaration order, and so are its binders.
    fn resolve_record(
        &self,
        rn: &str,
        entries: &[Vec<PatItem>],
        span: Span,
        binds: &mut Vec<(String, Type)>,
    ) -> Res<Pat> {
        let rd = &self.records[rn];
        let fields = || {
            rd.fields
                .iter()
                .map(FieldDef::signature)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut found: Vec<Option<(Vec<Pat>, Vec<(String, Type)>)>> = vec![None; rd.fields.len()];
        for entry in entries {
            let mut key = Vec::new();
            let mut slot_items = Vec::new();
            for item in entry {
                match item {
                    PatItem::Word(w) => key.push(w.as_str()),
                    PatItem::Bind(_) | PatItem::Nested(_) => {
                        key.push("_");
                        slot_items.push(item);
                    }
                    PatItem::Wild | PatItem::Record(_) => {
                        return Err(Diag::new(
                            span,
                            "a field pattern holds words and slots only; write `(_)` to ignore a slot",
                        ));
                    }
                }
            }
            let key = key.join(" ");
            let Some(fi) = rd.fields.iter().position(|d| d.key == key) else {
                return Err(Diag::new(
                    span,
                    format!(
                        "record `{rn}` has no field pattern `{key}`; its fields are: {}",
                        fields()
                    ),
                ));
            };
            let mut local = Vec::new();
            let mut pats = Vec::new();
            for (slot, item) in rd.fields[fi].slots.iter().zip(slot_items) {
                pats.push(self.resolve_slot(&slot.ty, item, span, &mut local)?);
            }
            if found[fi].replace((pats, local)).is_some() {
                return Err(Diag::new(
                    span,
                    format!("field `{}` is listed twice", rd.fields[fi].signature()),
                ));
            }
        }
        let missing: Vec<String> = rd
            .fields
            .iter()
            .zip(&found)
            .filter(|(_, f)| f.is_none())
            .map(|(d, _)| d.signature())
            .collect();
        if !missing.is_empty() {
            return Err(Diag::new(
                span,
                format!(
                    "destructuring `{rn}` must list every field; missing: {}",
                    missing.join(", ")
                ),
            ));
        }
        let mut args = Vec::new();
        for (pats, local) in found.into_iter().flatten() {
            args.extend(pats);
            binds.extend(local);
        }
        Ok(Pat::Ctor(0, args))
    }

    fn sig(&self) -> Sig<'_> {
        Sig {
            enums: &self.enums,
            records: &self.records,
        }
    }

    /// Resolves a pattern against `ty`: a record pattern for a record, variant
    /// words and slots for an enum. Slots hold binders, `(_)`, or nested
    /// patterns, to any depth. Binders are appended to `binds` in the order
    /// the matcher binds them.
    fn resolve(
        &self,
        ty: &Type,
        items: &[PatItem],
        span: Span,
        binds: &mut Vec<(String, Type)>,
    ) -> Res<Pat> {
        match (ty, items) {
            (Type::Record(rn), [PatItem::Record(entries)]) => {
                self.resolve_record(rn, entries, span, binds)
            }
            (Type::Record(rn), _) => {
                let rd = &self.records[rn];
                let fields: Vec<String> = rd.fields.iter().map(FieldDef::signature).collect();
                Err(Diag::new(
                    span,
                    format!(
                        "`{rn}` is a record, so the pattern must be a record pattern `{{ field (name), ... }}`; its fields are: {}",
                        fields.join(", ")
                    ),
                ))
            }
            (Type::Enum(..), _) => self.resolve_variant(ty, items, span, binds),
            _ => Err(Diag::new(
                span,
                format!(
                    "this slot has type {ty}, which is not an enum or record, so it cannot hold a nested pattern; use a `(name)` binder or `(_)`"
                ),
            )),
        }
    }

    fn resolve_slot(
        &self,
        slot: &Type,
        item: &PatItem,
        span: Span,
        binds: &mut Vec<(String, Type)>,
    ) -> Res<Pat> {
        Ok(match item {
            PatItem::Bind(n) if n == "_" => Pat::Any(None),
            PatItem::Bind(n) => {
                // A lone word naming a nullary variant of the slot's enum is
                // that variant, not a binder.
                let nullary = match slot {
                    Type::Enum(se, _) => self.enums[se]
                        .variants
                        .iter()
                        .position(|v| matches!(v.parts.as_slice(), [PartT::Word(w)] if w == n)),
                    _ => None,
                };
                match nullary {
                    Some(vi) => Pat::Ctor(vi, vec![]),
                    None => {
                        binds.push((n.clone(), slot.clone()));
                        Pat::Any(Some(n.clone()))
                    }
                }
            }
            PatItem::Nested(inner) => self.resolve(slot, inner, span, binds)?,
            _ => unreachable!("slots hold Bind or Nested"),
        })
    }

    fn resolve_variant(
        &self,
        ty: &Type,
        items: &[PatItem],
        span: Span,
        binds: &mut Vec<(String, Type)>,
    ) -> Res<Pat> {
        let Type::Enum(en, targs) = ty else {
            unreachable!("resolve dispatches enums here")
        };
        let ed = &self.enums[en];
        let pmap = enum_params(&self.enums, en, targs);
        let found = ed.variants.iter().position(|v| {
            v.parts.len() == items.len()
                && v.parts.iter().zip(items).all(|(p, q)| match (p, q) {
                    (PartT::Word(a), PatItem::Word(b)) => a == b,
                    (PartT::Param(..), PatItem::Bind(_) | PatItem::Nested(_)) => true,
                    _ => false,
                })
        });
        let Some(vi) = found else {
            let vs: Vec<String> = ed.variants.iter().map(|v| label(&v.parts)).collect();
            return Err(Diag::new(
                span,
                format!(
                    "no variant of `{en}` matches this pattern; variants are: {}",
                    vs.join(" | ")
                ),
            ));
        };
        let mut args = Vec::new();
        for (p, q) in ed.variants[vi].parts.iter().zip(items) {
            if let PartT::Param(_, slot) = p {
                let slot = subst(slot, &pmap, false);
                args.push(self.resolve_slot(&slot, q, span, binds)?);
            }
        }
        Ok(Pat::Ctor(vi, args))
    }

    fn matching(&self, scrut: &Expr, arms: &[Arm], span: Span, sc: &mut Scope) -> Res<(Ir, Type)> {
        let (sir, sty) = self.check(scrut, sc)?;
        if !matches!(sty, Type::Enum(..) | Type::Record(_)) {
            return Err(Diag::new(
                scrut.span,
                format!(
                    "MATCH needs an enum or record value (or Option/Result), found {sty}; use IF for Boolean"
                ),
            ));
        }
        let tys = [sty.clone()];
        let mut rows: Vec<Vec<Pat>> = Vec::new();
        let mut out: Option<Type> = None;
        let mut irs = Vec::new();
        for arm in arms {
            let mut binds = Vec::new();
            let pat = if arm.pat == [PatItem::Wild] {
                Pat::Any(None)
            } else {
                self.resolve(&sty, &arm.pat, arm.span, &mut binds)?
            };
            if !useful(&self.sig(), &rows, std::slice::from_ref(&pat), &tys) {
                return Err(Diag::new(
                    arm.span,
                    "this arm is unreachable: earlier arms already handle every value it matches",
                ));
            }
            rows.push(vec![pat.clone()]);
            let mark = sc.len();
            sc.extend(binds);
            let res = self.check(&arm.body, sc);
            sc.truncate(mark);
            let (bir, bt) = res?;
            out = Some(match &out {
                None => bt,
                Some(prev) => join(prev, &bt).ok_or_else(|| {
                    Diag::new(
                        arm.body.span,
                        format!(
                            "MATCH arms must have the same type: earlier arms are {prev}, this one is {bt}"
                        ),
                    )
                })?,
            });
            irs.push(IrArm { pat, body: bir });
        }
        let missing = witnesses(&self.sig(), &rows, &tys);
        if !missing.is_empty() {
            let shown: Vec<String> = missing
                .iter()
                .map(|w| render(&self.sig(), &sty, &w[0]))
                .collect();
            return Err(Diag::new(
                span,
                format!("non-exhaustive MATCH; missing: {}", shown.join(" | ")),
            ));
        }
        Ok((Ir::Match(Box::new(sir), irs), out.unwrap_or(Type::Unit)))
    }

    fn matches(parts: &[PartT], items: &[SeqItem]) -> bool {
        parts.len() == items.len()
            && parts.iter().zip(items).all(|(p, i)| match (p, i) {
                (PartT::Word(a), SeqItem::Word(b, _)) => a == b,
                (PartT::Param(..) | PartT::Tuple(_), _) => true,
                _ => false,
            })
    }

    /// Unifies an argument type with a declared parameter type, binding
    /// generic parameters in `m`. An argument may be a subtype of the
    /// parameter; a generic parameter binds to the join of its arguments.
    fn unify(&self, p: &Type, a: &Type, m: &mut HashMap<String, Type>) -> bool {
        match (p, a) {
            (_, Type::Never) => true,
            (Type::Param(n), _) => {
                if matches!(a, Type::Struct(_) | Type::Lambda) {
                    return false;
                }
                let joined = match m.get(n) {
                    None => Some(a.clone()),
                    Some(b) => join(b, a),
                };
                match joined {
                    Some(j) => {
                        m.insert(n.clone(), j);
                        true
                    }
                    None => false,
                }
            }
            (Type::List(x), Type::List(y)) | (Type::Set(x), Type::Set(y)) => self.unify(x, y, m),
            (Type::Function(p1, r1), Type::Function(p2, r2)) => {
                self.unify(p1, p2, m) && self.unify(r1, r2, m)
            }
            (Type::Tuple(xs), Type::Tuple(ys)) => {
                xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| self.unify(x, y, m))
            }
            (Type::Enum(n, xs), Type::Enum(k, ys)) => {
                n == k
                    && xs.len() == ys.len()
                    && xs.iter().zip(ys).all(|(x, y)| self.unify(x, y, m))
            }
            _ => self.fits(a, p),
        }
    }

    /// Is `d` callable from the current file? `qual` is the `ns::` in front of
    /// the call, if any: a qualified call sees only that namespace, an
    /// unqualified one sees the prelude and the file's own namespace.
    fn visible(&self, d: &FnDef, qual: Option<&str>) -> bool {
        match qual {
            Some(q) => d.ns.as_deref() == Some(q),
            None => {
                d.ns.is_none()
                    || d.ns == self.cur_ns
                    || d.ns
                        .as_ref()
                        .is_some_and(|n| self.open.borrow().contains(n))
            }
        }
    }

    /// Enters an `ns:: { ... }` block.
    pub fn open_scope(&self, ns: &str, span: Span) -> Res<()> {
        self.known_namespace(ns, span)?;
        self.open.borrow_mut().push(ns.to_string());
        Ok(())
    }

    pub fn close_scope(&self) {
        self.open.borrow_mut().pop();
    }

    fn seq(
        &self,
        items: &[SeqItem],
        span: Span,
        sc: &mut Scope,
        qual: Option<&str>,
    ) -> Res<(Ir, Type)> {
        if let ([SeqItem::Word(w, _)], None) = (items, qual) {
            match w.as_str() {
                "true" => return Ok((Ir::Lit(Value::Bool(true)), Type::Bool)),
                "false" => return Ok((Ir::Lit(Value::Bool(false)), Type::Bool)),
                _ => {}
            }
            if let Some((_, t)) = sc.iter().rev().find(|(n, _)| n == w) {
                return Ok((Ir::Var(w.clone()), t.clone()));
            }
        }
        let mut cands: Vec<usize> = (0..self.defs.len())
            .filter(|&i| {
                self.visible(&self.defs[i], qual) && Self::matches(&self.defs[i].parts, items)
            })
            .collect();
        // Inside `ns:: { ... }`, the opened namespaces win over names from outside.
        if qual.is_none() {
            let open = self.open.borrow();
            let inner: Vec<usize> = cands
                .iter()
                .copied()
                .filter(|&i| self.defs[i].ns.as_ref().is_some_and(|n| open.contains(n)))
                .collect();
            if !inner.is_empty() {
                cands = inner;
            }
        }
        if cands.is_empty() {
            return Err(self.no_match(items, span, sc, qual));
        }
        // Each argument is inferred once, on first use, independent of the
        // candidate it will be tested against.
        let mut args: Vec<Option<Res<(Ir, Type)>>> = vec![None; items.len()];
        let mut passing: Vec<(usize, HashMap<String, Type>, HashMap<usize, Ir>)> = Vec::new();
        let mut failures: Vec<Diag> = Vec::new();
        'cand: for &id in &cands {
            let def = &self.defs[id];
            let mut map: HashMap<String, Type> = HashMap::new();
            let mut lams: HashMap<usize, Ir> = HashMap::new();
            for (i, (part, item)) in def.parts.iter().zip(items).enumerate() {
                let Some((pname, pty)) = part.slot() else {
                    continue;
                };
                let pty = &pty;
                let (arg_span, arg) = match item {
                    SeqItem::Atom(e) => (e.span, e.clone()),
                    SeqItem::Word(w, s) => (
                        *s,
                        Expr {
                            k: ExprKind::Seq(vec![SeqItem::Word(w.clone(), *s)]),
                            span: *s,
                        },
                    ),
                };
                let inferred = args[i].get_or_insert_with(|| self.infer(&arg, sc));
                let aty = match inferred {
                    Ok((_, t)) => t.clone(),
                    Err(d) => {
                        failures.push(d.clone());
                        continue 'cand;
                    }
                };
                if matches!(aty, Type::Lambda)
                    && let Type::Function(want, ret) = pty
                    && let ExprKind::Lambda(x, body) = &arg.k
                {
                    if unbound_param(want, &map) {
                        failures.push(Diag::new(
                            arg_span,
                            format!(
                                "the parameter type of `({x}) => ...` is not known yet: `{}` is only fixed by an argument written after the lambda; move the lambda after it",
                                label(&def.parts)
                            ),
                        ));
                        continue 'cand;
                    }
                    sc.push((x.clone(), subst(want, &map, false)));
                    let res = self.check(body, sc);
                    sc.pop();
                    match res {
                        Ok((bir, bt)) if self.unify(ret, &bt, &mut map) => {
                            lams.insert(i, Ir::Lambda(x.clone(), Box::new(bir)));
                            continue;
                        }
                        Ok((_, bt)) => {
                            failures.push(Diag::new(
                                body.span,
                                self.mismatch(
                                    &format!(
                                        "the body of the lambda for `{pname}` of `{}`",
                                        label(&def.parts)
                                    ),
                                    &subst(ret, &map, true),
                                    &bt,
                                ),
                            ));
                            continue 'cand;
                        }
                        Err(d) => {
                            failures.push(d);
                            continue 'cand;
                        }
                    }
                }
                if !self.unify(pty, &aty, &mut map) {
                    failures.push(Diag::new(
                        arg_span,
                        self.mismatch(
                            &format!("argument `{pname}` of `{}`", label(&def.parts)),
                            &subst(pty, &map, true),
                            &aty,
                        ),
                    ));
                    continue 'cand;
                }
            }
            passing.push((id, map, lams));
        }
        let shown_args = || {
            let tys: Vec<String> = args
                .iter()
                .flatten()
                .filter_map(|r| r.as_ref().ok())
                .map(|(_, t)| t.to_string())
                .collect();
            format!("({})", tys.join(", "))
        };
        let candidates = |ids: &mut dyn Iterator<Item = usize>| {
            ids.map(|i| {
                let mut line = format!("\n    {}", label(&self.defs[i].parts));
                for part in &self.defs[i].parts {
                    if let PartT::Param(_, Type::Record(rn)) = part {
                        let fields: Vec<String> = self.records[rn.as_str()]
                            .fields
                            .iter()
                            .map(|f| {
                                let opt = f.slots.iter().all(|s| s.default.is_some());
                                format!("{}{}", f.signature(), if opt { " (optional)" } else { "" })
                            })
                            .collect();
                        let fields = if fields.is_empty() {
                            "none".to_string()
                        } else {
                            fields.join(", ")
                        };
                        line.push_str(&format!("\n      `{rn}` fields: {fields}"));
                    }
                }
                line
            })
            .collect::<String>()
        };
        match passing.len() {
            1 => {
                let (id, map, mut lams) = passing.pop().unwrap();
                let def = &self.defs[id];
                let mut irs = Vec::new();
                for (i, part) in def.parts.iter().enumerate() {
                    if let Some((_, pty)) = part.slot()
                        && let Some(Ok((ir, aty))) = args[i].take()
                    {
                        irs.push(match lams.remove(&i) {
                            Some(lam) => lam,
                            None => self.fit(ir, &aty, &pty),
                        });
                    }
                }
                Ok((Ir::Call(id, irs), subst(&def.ret, &map, false)))
            }
            0 if cands.len() == 1 || failures.len() == 1 => Err(failures.swap_remove(0)),
            0 => Err(Diag::new(
                span,
                format!(
                    "no overload of `{}` accepts argument types {}. Candidates:{}\n  Fix: change an argument so its type matches one candidate; record literals can be fixed with `{{ ... }} : Record Name`.",
                    seq_text(items),
                    shown_args(),
                    {
                        // Every failed candidate pushed exactly one reason.
                        let mut reasons = failures.iter();
                        cands
                            .iter()
                            .map(|&i| {
                                let line = candidates(&mut std::iter::once(i));
                                match reasons.next() {
                                    Some(d) => format!(
                                        "{line}\n      rejected: {}",
                                        d.msg.replace('\n', "\n      ")
                                    ),
                                    None => line,
                                }
                            })
                            .collect::<String>()
                    }
                ),
            )),
            _ => Err(Diag::new(
                span,
                format!(
                    "ambiguous call `{}` with argument types {}: more than one overload fits:{}\n  Fix: annotate a record literal with `{{ ... }} : Record Name`, or give the arguments more specific types.",
                    seq_text(items),
                    shown_args(),
                    candidates(&mut passing.iter().map(|(i, ..)| *i))
                ),
            )),
        }
    }

    fn no_match(&self, items: &[SeqItem], span: Span, sc: &Scope, qual: Option<&str>) -> Diag {
        // The call exists, but in a namespace the file did not qualify.
        if qual.is_none()
            && let Some(d) = self
                .defs
                .iter()
                .find(|d| !self.visible(d, None) && Self::matches(&d.parts, items))
            && let Some(ns) = &d.ns
        {
            return Diag::new(
                span,
                format!(
                    "`{}` is declared in namespace `{ns}`; callers outside it must qualify the call: `{ns}::{}`, or wrap the call in a scope block: `{ns}:: {{ ... }}`",
                    label(&d.parts),
                    seq_text(items)
                ),
            );
        }
        if let ([SeqItem::Word(w, s)], None) = (items, qual) {
            let vars: Vec<&str> = sc
                .iter()
                .map(|(n, _)| n.as_str())
                .filter(|n| *n != "_")
                .collect();
            return Diag::new(
                *s,
                format!(
                    "unknown name `{w}`: it is not a variable in scope ({}) and not a function without parameters",
                    if vars.is_empty() {
                        "none".to_string()
                    } else {
                        vars.join(", ")
                    }
                ),
            );
        }
        let words: Vec<&str> = items
            .iter()
            .filter_map(|i| {
                if let SeqItem::Word(w, _) = i {
                    Some(w.as_str())
                } else {
                    None
                }
            })
            .collect();
        let mut cands: Vec<String> = self
            .defs
            .iter()
            .filter(|d| self.visible(d, qual))
            .filter(|d| {
                d.parts
                    .iter()
                    .any(|p| matches!(p, PartT::Word(w) if words.contains(&w.as_str())))
            })
            .map(|d| label(&d.parts))
            .collect();
        cands.truncate(6);
        let scope = qual.map_or(String::new(), |q| format!(" in namespace `{q}`"));
        let mut msg = format!("no function matches `{}`{scope}.", seq_source(items));
        msg.push_str(
            " Each argument must be a single token, a quoted string, or a parenthesised call.",
        );
        if words.is_empty() {
            msg.push_str(&format!(
                " This is {} arguments side by side with no function name between them. \
                 To make a list write `[a, b]`; to combine two strings use `join (a) with (b)`; \
                 otherwise put the function's name words between the arguments.",
                items.len()
            ));
        } else {
            msg.push_str(&format!(
                " The function words used are: {}. Run `:symbols` in the REPL (or `resh libs show <name>`) to see declared signatures.",
                words.join(", ")
            ));
        }
        if !cands.is_empty() {
            msg.push_str(&format!(" Similar signatures: {}", cands.join(" | ")));
        }
        Diag::new(span, msg)
    }
}

#[cfg(test)]
mod strip_qualifier_tests {
    use super::strip_qualifier;

    #[test]
    fn strips_only_whole_qualifiers() {
        assert_eq!(
            strip_qualifier("find (root: String) with (options: fd::Options)", "fd::"),
            "find (root: String) with (options: Options)"
        );
        assert_eq!(strip_qualifier("List (fd::Entry)", "fd::"), "List (Entry)");
        assert_eq!(
            strip_qualifier("xfd::T and a::fd::T", "fd::"),
            "xfd::T and a::fd::T"
        );
    }
}
