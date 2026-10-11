//! Declaration table, overload resolution and type checking.
//!
//! Type checking is bottom-up and one-way: the type of an expression depends
//! only on its children, never on where it is used. Parsing leaves every call
//! as a flat `Seq` of words/atoms. Because every argument is exactly one
//! token, a call can only match a signature with the same length and the same
//! words in the word positions. Several signatures may share those words and
//! differ in parameter types (overloading); the arguments are inferred once
//! and the overload is chosen by testing `argument type <: parameter type`.

use std::collections::HashMap;
use std::fmt;

use crate::ast::*;
use crate::eval::{NATIVES, Value};
use crate::lexer::{Diag, Res, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    List(Box<Type>),
    Record(String),
    Enum(String, Vec<Type>),
    Param(String),
    /// The bottom type: a subtype of everything. The element type of `[]`,
    /// the payload of `none`, and any generic parameter nothing constrains.
    Never,
    /// The inferred type of a `{ ... }` literal; it only becomes a `Record`
    /// when it meets a record parameter or a `: Record Name` annotation.
    Struct(Vec<(String, Type)>),
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
            Type::Record(n) => write!(f, "{n}"),
            Type::Enum(n, args) if args.is_empty() => write!(f, "{n}"),
            Type::Enum(n, args) if n == "Result" && args.len() == 2 => {
                write!(f, "Result ({}) Or ({})", args[0], args[1])
            }
            Type::Enum(n, args) => {
                write!(f, "{n}")?;
                args.iter().try_for_each(|t| write!(f, " ({t})"))
            }
            Type::Param(p) => write!(f, "{p}"),
            Type::Never => write!(f, "Never"),
            Type::Struct(fs) => {
                write!(f, "record literal {{ ")?;
                for (i, (k, t)) in fs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {t}")?;
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
        (Type::List(x), Type::List(y)) => sub(x, y),
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
        (Type::List(x), Type::List(y)) => overlap(x, y),
        (Type::Enum(n, xs), Type::Enum(m, ys)) => {
            n == m && xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| overlap(x, y))
        }
        _ => a == b,
    }
}

/// Substitutes bound parameters; `keep_unbound` leaves unbound ones as-is
/// (for messages), otherwise they become `Never`.
fn subst(t: &Type, m: &HashMap<String, Type>, keep_unbound: bool) -> Type {
    match t {
        Type::Param(n) => match m.get(n) {
            Some(x) => x.clone(),
            None if keep_unbound => t.clone(),
            None => Type::Never,
        },
        Type::List(x) => Type::List(Box::new(subst(x, m, keep_unbound))),
        Type::Enum(n, a) => Type::Enum(
            n.clone(),
            a.iter().map(|x| subst(x, m, keep_unbound)).collect(),
        ),
        other => other.clone(),
    }
}

fn opt(t: Type) -> Type {
    Type::Enum("Option".into(), vec![t])
}

#[derive(Debug, Clone)]
pub enum PartT {
    Word(String),
    Param(String, Type),
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
    Field {
        idx: usize,
    },
    RecordNew,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub parts: Vec<PartT>,
    pub ret: Type,
    pub kind: FnKind,
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
}

#[derive(Debug, Clone)]
pub struct FieldDef {
    pub key: String,
    pub ty: Type,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct RecordDef {
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone)]
pub struct IrArm {
    pub variant: Option<usize>,
    pub binds: Vec<Option<String>>,
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
    MkRecord(String, Vec<Ir>),
    /// A `{ ... }` literal not yet fitted to a record; never reaches `eval`.
    Struct(Vec<(String, Ir)>),
    If(Box<Ir>, Box<Ir>, Box<Ir>),
    Match(Box<Ir>, Vec<IrArm>),
    Pipe(Box<Ir>, Box<Ir>),
}

pub type Scope = Vec<(String, Type)>;

#[derive(Debug, Clone)]
pub struct Checker {
    pub defs: Vec<FnDef>,
    pub enums: HashMap<String, EnumDef>,
    pub records: HashMap<String, RecordDef>,
}

pub fn label(parts: &[PartT]) -> String {
    parts
        .iter()
        .map(|p| match p {
            PartT::Word(w) => w.clone(),
            PartT::Param(n, t) => format!("({n}: {t})"),
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

fn count_placeholders(e: &Expr) -> usize {
    match &e.k {
        ExprKind::Placeholder => 1,
        ExprKind::Seq(items) => items
            .iter()
            .map(|i| match i {
                SeqItem::Atom(e) => count_placeholders(e),
                _ => 0,
            })
            .sum(),
        ExprKind::List(xs) => xs.iter().map(count_placeholders).sum(),
        ExprKind::Brace(fs) => fs
            .iter()
            .map(|f| f.value.as_ref().map_or(0, count_placeholders))
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
        ExprKind::Ascribe(e, _) => count_placeholders(e),
        // The right side of a nested pipe binds its own `_`.
        ExprKind::Pipe(l, _) => count_placeholders(l),
        _ => 0,
    }
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
                        parts: parts.clone(),
                        ret: ret.clone(),
                        kind: FnKind::Variant {
                            enum_name: name.to_string(),
                            idx,
                        },
                    });
                    variants.push(VariantDef { parts, fn_id: id });
                }
                ck.enums.insert(
                    name.to_string(),
                    EnumDef {
                        params: params.iter().map(|s| s.to_string()).collect(),
                        variants,
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
            "Option _" => arity(1, Type::Enum("Option".into(), args.clone())),
            "Result _ Or _" => arity(2, Type::Enum("Result".into(), args.clone())),
            n if n.len() == 1 && n.chars().all(|c| c.is_ascii_uppercase()) => {
                Ok(Type::Param(n.to_string()))
            }
            n if self.records.contains_key(n) => arity(0, Type::Record(n.to_string())),
            n if self.enums.contains_key(n) => arity(0, Type::Enum(n.to_string(), vec![])),
            n => Err(Diag::new(t.span, format!("unknown type `{n}`"))),
        }
    }

    fn mk_parts(&self, sig: &SigAst, self_ty: Option<&Type>) -> Res<Vec<PartT>> {
        let mut out = Vec::new();
        for p in &sig.parts {
            match p {
                Part::Word(w) => out.push(PartT::Word(w.clone())),
                Part::Param { name, ty: Some(t) } => {
                    out.push(PartT::Param(name.clone(), self.resolve_ty(t)?))
                }
                Part::Param { name, ty: None } => match self_ty {
                    Some(st) if name == "self" => out.push(PartT::Param(name.clone(), st.clone())),
                    _ => {
                        return Err(Diag::new(
                            sig.span,
                            format!("parameter `{name}` needs a type, e.g. `({name}: Path)`"),
                        ));
                    }
                },
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
            let same = d.parts.len() == parts.len()
                && d.parts.iter().zip(&parts).all(|(a, b)| match (a, b) {
                    (PartT::Word(x), PartT::Word(y)) => x == y,
                    (PartT::Param(_, x), PartT::Param(_, y)) => overlap(x, y),
                    _ => false,
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
        self.defs.push(FnDef { parts, ret, kind });
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
                Decl::Record { name, span, .. } => {
                    self.type_name_free(name, *span)?;
                    self.records
                        .insert(name.clone(), RecordDef { fields: vec![] });
                }
                Decl::Enum { name, span, .. } => {
                    self.type_name_free(name, *span)?;
                    self.enums.insert(
                        name.clone(),
                        EnumDef {
                            params: vec![],
                            variants: vec![],
                        },
                    );
                }
                _ => {}
            }
        }
        let mut pending = Vec::new();
        for d in decls {
            match d {
                Decl::Record { name, fields, span } => {
                    let rty = Type::Record(name.clone());
                    let mut ctor: Vec<PartT> = name
                        .split(' ')
                        .map(|w| PartT::Word(w.to_string()))
                        .collect();
                    ctor.push(PartT::Param("fields".into(), rty.clone()));
                    self.add_def(ctor, rty.clone(), FnKind::RecordNew, *span)?;
                    let mut fds = Vec::new();
                    for f in fields {
                        let mut parts = self.mk_parts(&f.sig, Some(&rty))?;
                        let others = parts
                            .iter()
                            .filter(|p| matches!(p, PartT::Param(n, _) if n != "self"))
                            .count();
                        if others > 0 {
                            return Err(Diag::new(
                                f.sig.span,
                                "record fields may only contain words and `(self)`",
                            ));
                        }
                        let selfs = parts
                            .iter()
                            .filter(|p| matches!(p, PartT::Param(..)))
                            .count();
                        if selfs > 1 {
                            return Err(Diag::new(
                                f.sig.span,
                                "a field may contain `(self)` only once",
                            ));
                        }
                        if selfs == 0 {
                            parts.push(PartT::Param("self".into(), rty.clone()));
                        }
                        let key = parts
                            .iter()
                            .filter_map(|p| {
                                if let PartT::Word(w) = p {
                                    Some(w.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" ");
                        let ty = self.resolve_ty(&f.ty)?;
                        let ret = if f.optional {
                            opt(ty.clone())
                        } else {
                            ty.clone()
                        };
                        self.add_def(parts, ret, FnKind::Field { idx: fds.len() }, f.sig.span)?;
                        fds.push(FieldDef {
                            key,
                            ty,
                            optional: f.optional,
                        });
                    }
                    self.records.get_mut(name).unwrap().fields = fds;
                }
                Decl::Enum { name, variants, .. } => {
                    let ety = Type::Enum(name.clone(), vec![]);
                    let mut vds = Vec::new();
                    for v in variants {
                        let parts = self.mk_parts(v, None)?;
                        let idx = vds.len();
                        let fid = self.add_def(
                            parts.clone(),
                            ety.clone(),
                            FnKind::Variant {
                                enum_name: name.clone(),
                                idx,
                            },
                            v.span,
                        )?;
                        vds.push(VariantDef { parts, fn_id: fid });
                    }
                    self.enums.get_mut(name).unwrap().variants = vds;
                }
                Decl::Function { sig, ret, body } => {
                    let parts = self.mk_parts(sig, None)?;
                    let rt = match ret {
                        Some(t) => self.resolve_ty(t)?,
                        None => Type::Unit,
                    };
                    let params = parts
                        .iter()
                        .filter_map(|p| {
                            if let PartT::Param(n, _) = p {
                                Some(n.clone())
                            } else {
                                None
                            }
                        })
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
                    if let FnBody::Block(_) = body {
                        pending.push((id, body, sig.span));
                    }
                }
            }
        }
        Ok(pending)
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
        let mut sc: Scope = def
            .parts
            .iter()
            .filter_map(|p| {
                if let PartT::Param(n, t) = p {
                    Some((n.clone(), t.clone()))
                } else {
                    None
                }
            })
            .collect();
        let FnBody::Block(stmts) = body else {
            unreachable!()
        };
        let (ir, t) = self.block(stmts, &mut sc)?;
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
            (Type::Struct(_), _) => false,
            _ => sub(a, b),
        }
    }

    fn field_ty(d: &FieldDef) -> Type {
        if d.optional {
            opt(d.ty.clone())
        } else {
            d.ty.clone()
        }
    }

    /// Why a record literal with these field types does not fit record `rn`.
    fn struct_problem(&self, fs: &[(String, Type)], rn: &str) -> Option<String> {
        let rd = &self.records[rn];
        for (k, t) in fs {
            match rd.fields.iter().find(|d| &d.key == k) {
                None => {
                    let known: Vec<&str> = rd.fields.iter().map(|d| d.key.as_str()).collect();
                    return Some(format!(
                        "record `{rn}` has no field `{k}`; its fields are: {}",
                        known.join(", ")
                    ));
                }
                Some(d) => {
                    let want = Self::field_ty(d);
                    if !self.fits(t, &want) {
                        let hint = if d.optional && self.fits(t, &d.ty) {
                            format!(
                                " (optional fields take an explicit `some ...`: `{k}: some ...`)"
                            )
                        } else {
                            String::new()
                        };
                        return Some(format!(
                            "field `{k}` of `{rn}` expects {want}, found {t}{hint}"
                        ));
                    }
                }
            }
        }
        for d in &rd.fields {
            if !d.optional && !fs.iter().any(|(k, _)| k == &d.key) {
                return Some(format!(
                    "record `{rn}` is missing required field `{}`",
                    d.key
                ));
            }
        }
        None
    }

    /// Turns an expression of type `ty` that `fits` `target` into one whose
    /// runtime shape is `target`'s: record literals become real records.
    fn fit(&self, ir: Ir, ty: &Type, target: &Type) -> Ir {
        let (Ir::Struct(given), Type::Struct(tys), Type::Record(rn)) = (&ir, ty, target) else {
            return ir;
        };
        let rd = &self.records[rn];
        let irs = rd
            .fields
            .iter()
            .map(|d| match given.iter().position(|(k, _)| k == &d.key) {
                Some(i) => self.fit(given[i].1.clone(), &tys[i].1, &Self::field_ty(d)),
                None => Ir::Call(self.variant_id("Option", 1), vec![]),
            })
            .collect();
        Ir::MkRecord(rn.clone(), irs)
    }

    fn reject_struct(t: &Type, span: Span) -> Res<()> {
        if matches!(t, Type::Struct(_)) {
            return Err(Diag::new(
                span,
                format!(
                    "a record literal has no type of its own here (it is {t}); annotate it: `{{ ... }} : Record Name`, or write the record name in front: `Record Name {{ ... }}`"
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
            ExprKind::Seq(items) => self.seq(items, e.span, sc),
            ExprKind::List(xs) => {
                let mut ty = Type::Never;
                let mut irs = Vec::new();
                for x in xs {
                    let (ir, t) = self.check(x, sc)?;
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
            ExprKind::Brace(fs) => self.brace(fs, sc),
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
        msg
    }

    fn variant_id(&self, en: &str, idx: usize) -> usize {
        self.enums[en].variants[idx].fn_id
    }

    /// A record literal is inferred from its fields alone. A bare field
    /// `flag` means `flag: some true`.
    fn brace(&self, fs: &[BraceField], sc: &mut Scope) -> Res<(Ir, Type)> {
        let mut irs: Vec<(String, Ir)> = Vec::new();
        let mut tys: Vec<(String, Type)> = Vec::new();
        for f in fs {
            let k = f.key.join(" ");
            if tys.iter().any(|(n, _)| n == &k) {
                return Err(Diag::new(f.span, format!("field `{k}` is given twice")));
            }
            let (ir, t) = match &f.value {
                Some(e) => self.infer(e, sc)?,
                None => (
                    Ir::Call(
                        self.variant_id("Option", 0),
                        vec![Ir::Lit(Value::Bool(true))],
                    ),
                    opt(Type::Bool),
                ),
            };
            irs.push((k.clone(), ir));
            tys.push((k, t));
        }
        Ok((Ir::Struct(irs), Type::Struct(tys)))
    }

    fn matching(&self, scrut: &Expr, arms: &[Arm], span: Span, sc: &mut Scope) -> Res<(Ir, Type)> {
        let (sir, sty) = self.check(scrut, sc)?;
        let Type::Enum(en, targs) = &sty else {
            return Err(Diag::new(
                scrut.span,
                format!(
                    "MATCH needs an enum value (or Option/Result), found {sty}; use IF for Boolean"
                ),
            ));
        };
        let ed = &self.enums[en];
        let pmap: HashMap<String, Type> = ed
            .params
            .iter()
            .cloned()
            .zip(targs.iter().cloned())
            .collect();
        let mut covered = vec![false; ed.variants.len()];
        let mut wildcard = false;
        let mut out: Option<Type> = None;
        let mut irs = Vec::new();
        for arm in arms {
            let mark = sc.len();
            let (variant, binds) = if arm.pat == [PatItem::Wild] {
                wildcard = true;
                (None, vec![])
            } else {
                let found = ed.variants.iter().position(|v| {
                    v.parts.len() == arm.pat.len()
                        && v.parts.iter().zip(&arm.pat).all(|(p, q)| match (p, q) {
                            (PartT::Word(a), PatItem::Word(b)) => a == b,
                            (PartT::Param(..), PatItem::Bind(_)) => true,
                            _ => false,
                        })
                });
                let Some(vi) = found else {
                    let vs: Vec<String> = ed.variants.iter().map(|v| label(&v.parts)).collect();
                    return Err(Diag::new(
                        arm.span,
                        format!(
                            "no variant of `{en}` matches this pattern; variants are: {}",
                            vs.join(" | ")
                        ),
                    ));
                };
                if covered[vi] {
                    return Err(Diag::new(
                        arm.span,
                        "this variant is already handled by an earlier arm",
                    ));
                }
                covered[vi] = true;
                let mut binds = Vec::new();
                for (p, q) in ed.variants[vi].parts.iter().zip(&arm.pat) {
                    if let (PartT::Param(_, ty), PatItem::Bind(name)) = (p, q) {
                        if name == "_" {
                            binds.push(None);
                        } else {
                            sc.push((name.clone(), subst(ty, &pmap, false)));
                            binds.push(Some(name.clone()));
                        }
                    }
                }
                (Some(vi), binds)
            };
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
            irs.push(IrArm {
                variant,
                binds,
                body: bir,
            });
        }
        if !wildcard {
            let missing: Vec<String> = ed
                .variants
                .iter()
                .zip(&covered)
                .filter(|(_, c)| !**c)
                .map(|(v, _)| label(&v.parts))
                .collect();
            if !missing.is_empty() {
                return Err(Diag::new(
                    span,
                    format!("non-exhaustive MATCH; missing: {}", missing.join(" | ")),
                ));
            }
        }
        Ok((Ir::Match(Box::new(sir), irs), out.unwrap_or(Type::Unit)))
    }

    fn matches(parts: &[PartT], items: &[SeqItem]) -> bool {
        parts.len() == items.len()
            && parts.iter().zip(items).all(|(p, i)| match (p, i) {
                (PartT::Word(a), SeqItem::Word(b, _)) => a == b,
                (PartT::Param(..), _) => true,
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
                if matches!(a, Type::Struct(_)) {
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
            (Type::List(x), Type::List(y)) => self.unify(x, y, m),
            (Type::Enum(n, xs), Type::Enum(k, ys)) => {
                n == k
                    && xs.len() == ys.len()
                    && xs.iter().zip(ys).all(|(x, y)| self.unify(x, y, m))
            }
            _ => self.fits(a, p),
        }
    }

    fn seq(&self, items: &[SeqItem], span: Span, sc: &mut Scope) -> Res<(Ir, Type)> {
        if let [SeqItem::Word(w, _)] = items {
            match w.as_str() {
                "true" => return Ok((Ir::Lit(Value::Bool(true)), Type::Bool)),
                "false" => return Ok((Ir::Lit(Value::Bool(false)), Type::Bool)),
                _ => {}
            }
            if let Some((_, t)) = sc.iter().rev().find(|(n, _)| n == w) {
                return Ok((Ir::Var(w.clone()), t.clone()));
            }
        }
        let cands: Vec<usize> = (0..self.defs.len())
            .filter(|&i| Self::matches(&self.defs[i].parts, items))
            .collect();
        if cands.is_empty() {
            return Err(self.no_match(items, span, sc));
        }
        // Each argument is inferred once, on first use, independent of the
        // candidate it will be tested against.
        let mut args: Vec<Option<Res<(Ir, Type)>>> = vec![None; items.len()];
        let mut passing: Vec<(usize, HashMap<String, Type>)> = Vec::new();
        let mut failures: Vec<Diag> = Vec::new();
        'cand: for &id in &cands {
            let def = &self.defs[id];
            let mut map: HashMap<String, Type> = HashMap::new();
            for (i, (part, item)) in def.parts.iter().zip(items).enumerate() {
                let PartT::Param(pname, pty) = part else {
                    continue;
                };
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
            passing.push((id, map));
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
            ids.map(|i| format!("\n    {}", label(&self.defs[i].parts)))
                .collect::<String>()
        };
        match passing.len() {
            1 => {
                let (id, map) = passing.pop().unwrap();
                let def = &self.defs[id];
                let mut irs = Vec::new();
                for (i, part) in def.parts.iter().enumerate() {
                    if let PartT::Param(_, pty) = part
                        && let Some(Ok((ir, aty))) = args[i].take()
                    {
                        irs.push(self.fit(ir, &aty, pty));
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
                    candidates(&mut cands.iter().copied())
                ),
            )),
            _ => Err(Diag::new(
                span,
                format!(
                    "ambiguous call `{}` with argument types {}: more than one overload fits:{}\n  Fix: annotate a record literal with `{{ ... }} : Record Name`, or give the arguments more specific types.",
                    seq_text(items),
                    shown_args(),
                    candidates(&mut passing.iter().map(|(i, _)| *i))
                ),
            )),
        }
    }

    fn no_match(&self, items: &[SeqItem], span: Span, sc: &Scope) -> Diag {
        if let [SeqItem::Word(w, s)] = items {
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
            .filter(|d| {
                d.parts
                    .iter()
                    .any(|p| matches!(p, PartT::Word(w) if words.contains(&w.as_str())))
            })
            .map(|d| label(&d.parts))
            .collect();
        cands.truncate(6);
        let mut msg = format!("no function matches `{}`.", seq_text(items));
        msg.push_str(
            " Each argument must be a single token, a quoted string, or a parenthesised call.",
        );
        if !cands.is_empty() {
            msg.push_str(&format!(" Similar signatures: {}", cands.join(" | ")));
        }
        Diag::new(span, msg)
    }
}
