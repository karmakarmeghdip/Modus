//! Semantic Type representation, unification, and substitution.

use crate::ast::{PrimitiveType, Span};
use crate::typechecker::error::{TypeError, TypeErrorKind};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// Unique identifier for an inferred type variable
pub type TypeVarId = u32;

/// Semantic representation of types in Modus
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Primitive types: u8..u64, i8..i64, f32, f64, bool, String, void
    Primitive(PrimitiveType),
    /// Unit type ()
    Unit,
    /// Type variable for type inference: ?T0, ?T1, ...
    Var(TypeVarId),
    /// Generic type parameter: T, E, Self
    GenericParam(String),
    /// Array type: [T]
    Array(Box<Type>),
    /// Function/closure type: (T1, T2) => R
    Function { params: Vec<Type>, ret: Box<Type> },
    /// Structural record type: { field1: T1, field2: T2 }
    /// BTreeMap ensures consistent ordering for structural equivalence
    Record(BTreeMap<String, Type>),
    /// Tuple type: (T1, T2)
    Tuple(Vec<Type>),
    /// Named type: Point, Option(T), Result(T, E), IO(T), etc.
    Named { name: String, args: Vec<Type> },
    /// Dynamic fat-pointer trait object: Drawable
    TraitObject(String),
}

impl Type {
    // Convenience constructors
    pub fn u8() -> Self {
        Type::Primitive(PrimitiveType::U8)
    }
    pub fn u16() -> Self {
        Type::Primitive(PrimitiveType::U16)
    }
    pub fn u32() -> Self {
        Type::Primitive(PrimitiveType::U32)
    }
    pub fn u64() -> Self {
        Type::Primitive(PrimitiveType::U64)
    }
    pub fn i8() -> Self {
        Type::Primitive(PrimitiveType::I8)
    }
    pub fn i16() -> Self {
        Type::Primitive(PrimitiveType::I16)
    }
    pub fn i32() -> Self {
        Type::Primitive(PrimitiveType::I32)
    }
    pub fn i64() -> Self {
        Type::Primitive(PrimitiveType::I64)
    }
    pub fn f32() -> Self {
        Type::Primitive(PrimitiveType::F32)
    }
    pub fn f64() -> Self {
        Type::Primitive(PrimitiveType::F64)
    }
    pub fn bool() -> Self {
        Type::Primitive(PrimitiveType::Bool)
    }
    pub fn string() -> Self {
        Type::Primitive(PrimitiveType::String)
    }
    pub fn void() -> Self {
        Type::Primitive(PrimitiveType::Void)
    }

    pub fn io(inner: Type) -> Self {
        Type::Named {
            name: "IO".to_string(),
            args: vec![inner],
        }
    }

    pub fn result(ok: Type, err: Type) -> Self {
        Type::Named {
            name: "Result".to_string(),
            args: vec![ok, err],
        }
    }

    pub fn option(inner: Type) -> Self {
        Type::Named {
            name: "Option".to_string(),
            args: vec![inner],
        }
    }

    pub fn pointer(inner: Type) -> Self {
        Type::Named {
            name: "Pointer".to_string(),
            args: vec![inner],
        }
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Type::Named { name, args } if name == "Pointer" && args.len() == 1)
    }

    pub fn unwrap_pointer(&self) -> Option<&Type> {
        match self {
            Type::Named { name, args } if name == "Pointer" && args.len() == 1 => Some(&args[0]),
            _ => None,
        }
    }

    pub fn cstring() -> Self {
        Type::Named {
            name: "CString".to_string(),
            args: vec![],
        }
    }

    pub fn is_cstring(&self) -> bool {
        matches!(self, Type::Named { name, .. } if name == "CString")
    }

    pub fn is_void(&self) -> bool {
        matches!(self, Type::Primitive(PrimitiveType::Void) | Type::Unit)
    }

    pub fn is_io(&self) -> bool {
        matches!(self, Type::Named { name, args } if name == "IO" && args.len() == 1)
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Type::Array(_))
    }

    pub fn unwrap_io(&self) -> Option<&Type> {
        match self {
            Type::Named { name, args } if name == "IO" && args.len() == 1 => Some(&args[0]),
            _ => None,
        }
    }

    pub fn is_result(&self) -> bool {
        matches!(self, Type::Named { name, args } if name == "Result" && args.len() == 2)
    }

    pub fn unwrap_result(&self) -> Option<(&Type, &Type)> {
        match self {
            Type::Named { name, args } if name == "Result" && args.len() == 2 => {
                Some((&args[0], &args[1]))
            }
            _ => None,
        }
    }

    pub fn is_option(&self) -> bool {
        matches!(self, Type::Named { name, args } if name == "Option" && args.len() == 1)
    }

    pub fn unwrap_option(&self) -> Option<&Type> {
        match self {
            Type::Named { name, args } if name == "Option" && args.len() == 1 => Some(&args[0]),
            _ => None,
        }
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::Primitive(
                PrimitiveType::U8
                    | PrimitiveType::U16
                    | PrimitiveType::U32
                    | PrimitiveType::U64
                    | PrimitiveType::I8
                    | PrimitiveType::I16
                    | PrimitiveType::I32
                    | PrimitiveType::I64
            )
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(
            self,
            Type::Primitive(PrimitiveType::F32 | PrimitiveType::F64)
        )
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Primitive(PrimitiveType::Bool))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Type::Primitive(PrimitiveType::String))
    }

    pub fn is_unsigned_integer(&self) -> bool {
        matches!(
            self,
            Type::Primitive(
                PrimitiveType::U8 | PrimitiveType::U16 | PrimitiveType::U32 | PrimitiveType::U64
            )
        )
    }

    /// Recursively check if a type variable occurs in this type
    pub fn contains_var(&self, var: TypeVarId) -> bool {
        match self {
            Type::Var(v) => *v == var,
            Type::Array(inner) => inner.contains_var(var),
            Type::Function { params, ret } => {
                params.iter().any(|p| p.contains_var(var)) || ret.contains_var(var)
            }
            Type::Record(fields) => fields.values().any(|f| f.contains_var(var)),
            Type::Tuple(elems) => elems.iter().any(|e| e.contains_var(var)),
            Type::Named { args, .. } => args.iter().any(|a| a.contains_var(var)),
            _ => false,
        }
    }

    /// Free type variables in this type
    pub fn free_vars(&self) -> HashSet<TypeVarId> {
        let mut vars = HashSet::new();
        self.collect_free_vars(&mut vars);
        vars
    }

    fn collect_free_vars(&self, vars: &mut HashSet<TypeVarId>) {
        match self {
            Type::Var(v) => {
                vars.insert(*v);
            }
            Type::Array(inner) => inner.collect_free_vars(vars),
            Type::Function { params, ret } => {
                for p in params {
                    p.collect_free_vars(vars);
                }
                ret.collect_free_vars(vars);
            }
            Type::Record(fields) => {
                for f in fields.values() {
                    f.collect_free_vars(vars);
                }
            }
            Type::Tuple(elems) => {
                for e in elems {
                    e.collect_free_vars(vars);
                }
            }
            Type::Named { args, .. } => {
                for a in args {
                    a.collect_free_vars(vars);
                }
            }
            _ => {}
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Primitive(p) => match p {
                PrimitiveType::U8 => write!(f, "u8"),
                PrimitiveType::U16 => write!(f, "u16"),
                PrimitiveType::U32 => write!(f, "u32"),
                PrimitiveType::U64 => write!(f, "u64"),
                PrimitiveType::I8 => write!(f, "i8"),
                PrimitiveType::I16 => write!(f, "i16"),
                PrimitiveType::I32 => write!(f, "i32"),
                PrimitiveType::I64 => write!(f, "i64"),
                PrimitiveType::F32 => write!(f, "f32"),
                PrimitiveType::F64 => write!(f, "f64"),
                PrimitiveType::Bool => write!(f, "bool"),
                PrimitiveType::String => write!(f, "String"),
                PrimitiveType::Void => write!(f, "void"),
            },
            Type::Unit => write!(f, "()"),
            Type::Var(v) => write!(f, "?T{v}"),
            Type::GenericParam(name) => write!(f, "{name}"),
            Type::Array(elem) => write!(f, "[{elem}]"),
            Type::Function { params, ret } => {
                let params_str = params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({params_str}) => {ret}")
            }
            Type::Record(fields) => {
                let fields_str = fields
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "{{ {fields_str} }}")
            }
            Type::Tuple(elems) => {
                let elems_str = elems
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({elems_str})")
            }
            Type::Named { name, args } => {
                if args.is_empty() {
                    write!(f, "{name}")
                } else {
                    let args_str = args
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "{name}({args_str})")
                }
            }
            Type::TraitObject(name) => write!(f, "{name}"),
        }
    }
}

/// Generator for fresh type variables
#[derive(Debug, Default, Clone)]
pub struct TypeVarGen {
    next_id: TypeVarId,
}

impl TypeVarGen {
    pub fn new() -> Self {
        Self { next_id: 0 }
    }

    pub fn fresh(&mut self) -> Type {
        let id = self.next_id;
        self.next_id += 1;
        Type::Var(id)
    }
}

/// Substitution mapping type variables to concrete types
#[derive(Debug, Clone, Default)]
pub struct Substitution {
    mapping: HashMap<TypeVarId, Type>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
        }
    }

    pub fn insert(&mut self, var: TypeVarId, ty: Type) {
        self.mapping.insert(var, ty);
    }

    pub fn get(&self, var: TypeVarId) -> Option<&Type> {
        self.mapping.get(&var)
    }

    /// Apply substitution recursively until a fixed point is reached
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(v) => {
                if let Some(substituted) = self.mapping.get(v) {
                    self.apply(substituted)
                } else {
                    Type::Var(*v)
                }
            }
            Type::Array(elem) => Type::Array(Box::new(self.apply(elem))),
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
            },
            Type::Record(fields) => {
                let mut new_fields = BTreeMap::new();
                for (k, v) in fields {
                    new_fields.insert(k.clone(), self.apply(v));
                }
                Type::Record(new_fields)
            }
            Type::Tuple(elems) => Type::Tuple(elems.iter().map(|e| self.apply(e)).collect()),
            Type::Named { name, args } => Type::Named {
                name: name.clone(),
                args: args.iter().map(|a| self.apply(a)).collect(),
            },
            _ => ty.clone(),
        }
    }

    /// Unify two types under this substitution, updating it
    pub fn unify(
        &mut self,
        t1: &Type,
        t2: &Type,
        span: Option<Span>,
        alias_expander: &dyn Fn(&str, &[Type]) -> Option<Type>,
    ) -> Result<(), TypeError> {
        let t1 = self.apply(t1);
        let t2 = self.apply(t2);

        if t1 == t2 {
            return Ok(());
        }

        // Check if t1 or t2 is a type alias that should be expanded
        if let Type::Named { name, args } = &t1
            && let Some(expanded) = alias_expander(name, args)
        {
            return self.unify(&expanded, &t2, span, alias_expander);
        }
        if let Type::Named { name, args } = &t2
            && let Some(expanded) = alias_expander(name, args)
        {
            return self.unify(&t1, &expanded, span, alias_expander);
        }

        match (&t1, &t2) {
            (Type::Var(v1), _) => self.bind_var(*v1, &t2, span),
            (_, Type::Var(v2)) => self.bind_var(*v2, &t1, span),

            (Type::Primitive(p1), Type::Primitive(p2)) if p1 == p2 => Ok(()),

            (Type::Array(e1), Type::Array(e2)) => self.unify(e1, e2, span, alias_expander),

            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::ArgCountMismatch {
                            expected: p1.len(),
                            found: p2.len(),
                        },
                        span,
                    ));
                }
                for (a, b) in p1.iter().zip(p2.iter()) {
                    self.unify(a, b, span, alias_expander)?;
                }
                self.unify(r1, r2, span, alias_expander)
            }

            (Type::Record(r1), Type::Record(r2)) => {
                // Structural equality: all fields in r1 must exist in r2 with unified types,
                // and no extra fields in either.
                for (k, v1) in r1 {
                    if let Some(v2) = r2.get(k) {
                        self.unify(v1, v2, span, alias_expander)?;
                    } else {
                        return Err(TypeError::new(
                            TypeErrorKind::MissingRecordField {
                                field: k.clone(),
                                record_type: t2.to_string(),
                            },
                            span,
                        ));
                    }
                }
                for k in r2.keys() {
                    if !r1.contains_key(k) {
                        return Err(TypeError::new(
                            TypeErrorKind::ExtraneousRecordField {
                                field: k.clone(),
                                record_type: t1.to_string(),
                            },
                            span,
                        ));
                    }
                }
                Ok(())
            }

            (Type::Tuple(t1_elems), Type::Tuple(t2_elems)) => {
                if t1_elems.len() != t2_elems.len() {
                    return Err(TypeError::new(
                        TypeErrorKind::TypeMismatch {
                            expected: t2.to_string(),
                            found: t1.to_string(),
                        },
                        span,
                    ));
                }
                for (a, b) in t1_elems.iter().zip(t2_elems.iter()) {
                    self.unify(a, b, span, alias_expander)?;
                }
                Ok(())
            }

            (Type::Named { name: n1, args: a1 }, Type::Named { name: n2, args: a2 })
                if n1 == n2 && a1.len() == a2.len() =>
            {
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    self.unify(arg1, arg2, span, alias_expander)?;
                }
                Ok(())
            }

            (Type::GenericParam(p1), Type::GenericParam(p2)) if p1 == p2 => Ok(()),

            // Unit and Void are compatible in Modus
            (Type::Unit, Type::Primitive(PrimitiveType::Void))
            | (Type::Primitive(PrimitiveType::Void), Type::Unit) => Ok(()),

            _ => Err(TypeError::new(
                TypeErrorKind::TypeMismatch {
                    expected: t2.to_string(),
                    found: t1.to_string(),
                },
                span,
            )),
        }
    }

    fn bind_var(&mut self, var: TypeVarId, ty: &Type, span: Option<Span>) -> Result<(), TypeError> {
        if let Type::Var(v2) = ty
            && var == *v2
        {
            return Ok(());
        }
        if ty.contains_var(var) {
            return Err(TypeError::new(TypeErrorKind::OccursCheckFailed, span));
        }
        self.mapping.insert(var, ty.clone());
        Ok(())
    }
}
