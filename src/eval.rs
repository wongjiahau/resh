//! Values, the tree-walking evaluator, native functions and rendering.

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path as FsPath;

use crate::check::{Checker, FnKind, Ir, PartT};

pub const NATIVES: &[&str] = &[
    "list_file",
    "list_file_in",
    "list_default",
    "list_in_default",
    "human_size",
    "read_file",
    "write_file",
    "move_path",
    "print",
    "count",
    "filter_ext",
    "lines_of",
    "join_with",
    "equals",
    "unwrap",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    List(Vec<Value>),
    /// record type name, field values in declaration order
    Record(String, Vec<Value>),
    /// enum name, variant index, arguments
    Variant(String, usize, Vec<Value>),
}

type Env = Vec<(String, Value)>;

pub struct Interp {
    pub ck: Checker,
}

fn ok(v: Value) -> Value {
    Value::Variant("Result".into(), 0, vec![v])
}
fn err(s: impl Into<String>) -> Value {
    Value::Variant("Result".into(), 1, vec![Value::Str(s.into())])
}
fn as_str(v: &Value) -> &str {
    match v {
        Value::Str(s) => s,
        _ => "",
    }
}
fn opt_bool(v: &Value) -> bool {
    matches!(v, Value::Variant(e, 0, a) if e == "Option" && a.first() == Some(&Value::Bool(true)))
}

impl Default for Interp {
    fn default() -> Self {
        Interp::new()
    }
}

impl Interp {
    pub fn new() -> Interp {
        Interp { ck: Checker::new() }
    }

    pub fn eval(&self, ir: &Ir, env: &mut Env) -> Result<Value, String> {
        match ir {
            Ir::Lit(v) => Ok(v.clone()),
            Ir::Struct(_) => Err("unresolved record literal (checker bug)".into()),
            Ir::Var(n) => env
                .iter()
                .rev()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("internal error: unbound variable `{n}`")),
            Ir::Let(n, e) => {
                let v = self.eval(e, env)?;
                env.push((n.clone(), v));
                Ok(Value::Unit)
            }
            Ir::Block(irs) => {
                let mark = env.len();
                let mut last = Value::Unit;
                for i in irs {
                    last = self.eval(i, env)?;
                }
                env.truncate(mark);
                Ok(last)
            }
            Ir::List(xs) => Ok(Value::List(
                xs.iter()
                    .map(|x| self.eval(x, env))
                    .collect::<Result<_, _>>()?,
            )),
            Ir::MkRecord(n, fs) => Ok(Value::Record(
                n.clone(),
                fs.iter()
                    .map(|x| self.eval(x, env))
                    .collect::<Result<_, _>>()?,
            )),
            Ir::If(c, t, f) => match self.eval(c, env)? {
                Value::Bool(true) => self.eval(t, env),
                _ => self.eval(f, env),
            },
            Ir::Pipe(l, r) => {
                let v = self.eval(l, env)?;
                let mark = env.len();
                env.push(("_".into(), v));
                let res = self.eval(r, env);
                env.truncate(mark);
                res
            }
            Ir::Match(s, arms) => {
                let v = self.eval(s, env)?;
                let Value::Variant(_, idx, args) = v else {
                    return Err("internal error: MATCH on a non-variant".into());
                };
                for arm in arms {
                    if arm.variant.is_none() || arm.variant == Some(idx) {
                        let mark = env.len();
                        let mut it = args.iter();
                        for b in &arm.binds {
                            let a = it.next().cloned().unwrap_or(Value::Unit);
                            if let Some(name) = b {
                                env.push((name.clone(), a));
                            }
                        }
                        let res = self.eval(&arm.body, env);
                        env.truncate(mark);
                        return res;
                    }
                }
                Err("internal error: no MATCH arm matched".into())
            }
            Ir::Call(id, args) => {
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval(a, env))
                    .collect::<Result<_, _>>()?;
                let def = &self.ck.defs[*id];
                match &def.kind {
                    FnKind::User { params, body } => {
                        let body = body
                            .as_ref()
                            .ok_or("internal error: function body missing")?;
                        let mut local: Env = params.iter().cloned().zip(vals).collect();
                        self.eval(body, &mut local)
                    }
                    FnKind::Native(n) => self.native(n, vals),
                    FnKind::Variant { enum_name, idx } => {
                        Ok(Value::Variant(enum_name.clone(), *idx, vals))
                    }
                    FnKind::Field { idx } => match vals.into_iter().next() {
                        Some(Value::Record(_, fs)) => Ok(fs[*idx].clone()),
                        _ => Err("internal error: field access on a non-record".into()),
                    },
                    FnKind::RecordNew => Ok(vals.into_iter().next().unwrap_or(Value::Unit)),
                }
            }
        }
    }

    // ---------- natives ----------

    fn native(&self, name: &str, a: Vec<Value>) -> Result<Value, String> {
        match name {
            "list_file" => Ok(self.list(".", Some(&a[0]))),
            "list_file_in" => Ok(self.list(as_str(&a[0]), Some(&a[1]))),
            "list_default" => Ok(self.list(".", None)),
            "list_in_default" => Ok(self.list(as_str(&a[0]), None)),
            "human_size" => match &a[0] {
                Value::Int(n) => Ok(Value::Str(human_size(*n))),
                _ => Err("human size: not an Int".into()),
            },
            "read_file" => Ok(match fs::read_to_string(as_str(&a[0])) {
                Ok(s) => ok(Value::Str(s)),
                Err(e) => err(format!("cannot read {}: {e}", as_str(&a[0]))),
            }),
            "write_file" => Ok(match fs::write(as_str(&a[1]), as_str(&a[0])) {
                Ok(()) => ok(Value::Unit),
                Err(e) => err(format!("cannot write {}: {e}", as_str(&a[1]))),
            }),
            "move_path" => Ok(match fs::rename(as_str(&a[0]), as_str(&a[1])) {
                Ok(()) => ok(Value::Unit),
                Err(e) => err(format!(
                    "cannot move {} to {}: {e}",
                    as_str(&a[0]),
                    as_str(&a[1])
                )),
            }),
            "print" => {
                println!("{}", self.render(&a[0], false));
                Ok(Value::Unit)
            }
            "count" => match &a[0] {
                Value::List(xs) => Ok(Value::Int(xs.len() as i64)),
                _ => Err("count: not a list".into()),
            },
            "filter_ext" => {
                let ext = as_str(&a[1]).trim_start_matches('.').to_string();
                let Value::List(xs) = &a[0] else {
                    return Err("filter: not a list".into());
                };
                Ok(Value::List(
                    xs.iter()
                        .filter(|e| match e {
                            Value::Record(_, f) => FsPath::new(as_str(&f[0]))
                                .extension()
                                .is_some_and(|x| x.to_string_lossy() == ext),
                            _ => false,
                        })
                        .cloned()
                        .collect(),
                ))
            }
            "lines_of" => Ok(Value::List(
                as_str(&a[0])
                    .lines()
                    .map(|l| Value::Str(l.to_string()))
                    .collect(),
            )),
            "join_with" => Ok(Value::Str(format!("{}{}", as_str(&a[0]), as_str(&a[1])))),
            "equals" => Ok(Value::Bool(a[0] == a[1])),
            "unwrap" => match &a[0] {
                Value::Variant(_, 0, args) => Ok(args[0].clone()),
                other => Err(format!("unwrap failed: {}", self.render(other, false))),
            },
            other => Err(format!(
                "internal error: native `{other}` is not implemented"
            )),
        }
    }

    fn list(&self, dir: &str, opts: Option<&Value>) -> Value {
        let mut o = ListOpts::default();
        if let Some(Value::Record(_, f)) = opts {
            o.recursive = opt_bool(&f[0]);
            o.hidden = opt_bool(&f[1]);
            o.itself = opt_bool(&f[2]);
            o.reverse = opt_bool(&f[3]);
            o.follow = opt_bool(&f[4]);
            if let Value::Variant(e, 0, a) = &f[5]
                && e == "Option"
                && let Some(Value::Variant(_, idx, _)) = a.first()
            {
                o.sort = *idx;
            }
        }
        let mut out = Vec::new();
        let root = FsPath::new(dir);
        let res = if o.itself {
            make_entry(root, o.follow).map(|e| out.push(e))
        } else {
            walk(root, &o, &mut out)
        };
        match res {
            Ok(()) => ok(Value::List(out)),
            Err(e) => err(format!("cannot list {dir}: {e}")),
        }
    }

    // ---------- rendering ----------

    pub fn render(&self, v: &Value, json: bool) -> String {
        if json {
            self.json(v)
        } else {
            self.pretty(v, 0)
        }
    }

    fn variant_label(&self, en: &str, idx: usize) -> String {
        self.ck.enums[en].variants[idx]
            .parts
            .iter()
            .filter_map(|p| {
                if let PartT::Word(w) = p {
                    Some(w.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn compact(&self, v: &Value) -> String {
        match v {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => format!("{f:?}"),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => format!("{s:?}"),
            Value::Unit => "unit".into(),
            Value::List(xs) => format!(
                "[{}]",
                xs.iter()
                    .map(|x| self.compact(x))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Value::Record(n, fs) => {
                if fs.is_empty() {
                    return format!("{n} {{}}");
                }
                let keys = &self.ck.records[n].fields;
                let body: Vec<String> = keys
                    .iter()
                    .zip(fs)
                    .map(|(k, x)| format!("{}: {}", k.key, self.compact(x)))
                    .collect();
                format!("{n} {{ {} }}", body.join(", "))
            }
            Value::Variant(en, idx, args) => {
                let mut it = args.iter();
                self.ck.enums[en].variants[*idx]
                    .parts
                    .iter()
                    .map(|p| match p {
                        PartT::Word(w) => w.clone(),
                        PartT::Param(..) => format!(
                            "({})",
                            it.next().map(|a| self.compact(a)).unwrap_or_default()
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }

    fn pretty(&self, v: &Value, ind: usize) -> String {
        let c = self.compact(v);
        if c.len() + ind <= 80 {
            return c;
        }
        let pad = " ".repeat(ind + 2);
        let end = " ".repeat(ind);
        match v {
            Value::List(xs) => {
                let rows: Vec<String> = xs
                    .iter()
                    .map(|x| format!("{pad}{}", self.pretty(x, ind + 2)))
                    .collect();
                format!("[\n{}\n{end}]", rows.join(",\n"))
            }
            Value::Record(n, fs) => {
                let keys = &self.ck.records[n].fields;
                let rows: Vec<String> = keys
                    .iter()
                    .zip(fs)
                    .map(|(k, x)| format!("{pad}{}: {}", k.key, self.pretty(x, ind + 2)))
                    .collect();
                format!("{n} {{\n{}\n{end}}}", rows.join(",\n"))
            }
            Value::Variant(en, idx, args) => {
                let mut it = args.iter();
                self.ck.enums[en].variants[*idx]
                    .parts
                    .iter()
                    .map(|p| match p {
                        PartT::Word(w) => w.clone(),
                        PartT::Param(..) => format!(
                            "({})",
                            it.next().map(|a| self.pretty(a, ind)).unwrap_or_default()
                        ),
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
            _ => c,
        }
    }

    fn json(&self, v: &Value) -> String {
        match v {
            Value::Int(n) => n.to_string(),
            Value::Float(f) => format!("{f:?}"),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => json_str(s),
            Value::Unit => "null".into(),
            Value::List(xs) => format!(
                "[{}]",
                xs.iter()
                    .map(|x| self.json(x))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Value::Record(n, fs) => {
                let keys = &self.ck.records[n].fields;
                let body: Vec<String> = keys
                    .iter()
                    .zip(fs)
                    .map(|(k, x)| format!("{}:{}", json_str(&k.key), self.json(x)))
                    .collect();
                format!("{{{}}}", body.join(","))
            }
            Value::Variant(en, idx, args) => match (en.as_str(), *idx) {
                ("Option", 0) => self.json(&args[0]),
                ("Option", _) => "null".into(),
                ("Result", 0) => format!("{{\"ok\":{}}}", self.json(&args[0])),
                ("Result", _) => format!("{{\"error\":{}}}", self.json(&args[0])),
                _ => {
                    let label = self.variant_label(en, *idx);
                    if args.is_empty() {
                        json_str(&label)
                    } else {
                        format!(
                            "{{\"variant\":{},\"args\":[{}]}}",
                            json_str(&label),
                            args.iter()
                                .map(|a| self.json(a))
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    }
                }
            },
        }
    }
}

pub fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Options for `list file`, decoded from the `List Options` record.
/// `sort` is the index of the `Sort Key` variant: name, size, modified time, extension, unsorted.
#[derive(Default)]
struct ListOpts {
    recursive: bool,
    hidden: bool,
    itself: bool,
    reverse: bool,
    follow: bool,
    sort: usize,
}

fn field_int(v: &Value, i: usize) -> i64 {
    match v {
        Value::Record(_, f) => match f.get(i) {
            Some(Value::Int(n)) => *n,
            _ => 0,
        },
        _ => 0,
    }
}

fn field_path(v: &Value) -> String {
    match v {
        Value::Record(_, f) => as_str(&f[0]).to_string(),
        _ => String::new(),
    }
}

fn extension_of(v: &Value) -> String {
    FsPath::new(&field_path(v))
        .extension()
        .map(|x| x.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn mode_string(kind: usize, mode: u32) -> String {
    let mut s = String::from(match kind {
        1 => 'd',
        2 => 'l',
        _ => '-',
    });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

fn human_size(n: i64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n}B")
    } else {
        format!("{v:.1}{}", UNITS[u])
    }
}

fn make_entry(path: &FsPath, follow: bool) -> std::io::Result<Value> {
    let lmeta = fs::symlink_metadata(path)?;
    let is_link = lmeta.file_type().is_symlink();
    let meta = if follow && is_link {
        fs::metadata(path).unwrap_or_else(|_| lmeta.clone())
    } else {
        lmeta.clone()
    };
    let kind = if meta.file_type().is_symlink() {
        2
    } else if meta.is_dir() {
        1
    } else {
        0
    };
    let target = if is_link {
        match fs::read_link(path) {
            Ok(t) => Value::Variant(
                "Option".into(),
                0,
                vec![Value::Str(t.to_string_lossy().to_string())],
            ),
            Err(_) => none(),
        }
    } else {
        none()
    };
    let shown = path.to_string_lossy().trim_start_matches("./").to_string();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| shown.clone());
    Ok(Value::Record(
        "Entry".into(),
        vec![
            Value::Str(shown),
            Value::Variant("Entry Kind".into(), kind, vec![]),
            Value::Int(if kind == 0 { meta.len() as i64 } else { 0 }),
            Value::Str(name),
            Value::Str(mode_string(kind, meta.mode())),
            Value::Int((meta.mode() & 0o7777) as i64),
            Value::Int(meta.nlink() as i64),
            Value::Int(meta.uid() as i64),
            Value::Int(meta.gid() as i64),
            Value::Int(meta.mtime()),
            Value::Int(meta.atime()),
            Value::Int(meta.ino() as i64),
            Value::Int(meta.blocks() as i64),
            target,
        ],
    ))
}

fn none() -> Value {
    Value::Variant("Option".into(), 1, vec![])
}

fn walk(dir: &FsPath, o: &ListOpts, out: &mut Vec<Value>) -> std::io::Result<()> {
    let mut level = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !o.hidden && entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        level.push(make_entry(&entry.path(), o.follow)?);
    }
    match o.sort {
        0 => level.sort_by_key(field_path),
        1 => level.sort_by_key(|e| (std::cmp::Reverse(field_int(e, 2)), field_path(e))),
        2 => level.sort_by_key(|e| (std::cmp::Reverse(field_int(e, 9)), field_path(e))),
        3 => level.sort_by_key(|e| (extension_of(e), field_path(e))),
        _ => {}
    }
    if o.reverse {
        level.reverse();
    }
    for e in level {
        let is_dir = matches!(&e, Value::Record(_, f) if matches!(f[1], Value::Variant(_, 1, _)));
        let path = field_path(&e);
        out.push(e);
        if o.recursive && is_dir {
            walk(FsPath::new(&path), o, out)?;
        }
    }
    Ok(())
}
