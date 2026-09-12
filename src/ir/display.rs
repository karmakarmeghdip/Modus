//! Pretty printing and formatting for ANF IR.

use crate::ast::Literal;
use crate::ir::node::*;
use std::fmt;

impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Atom::Var(name) => write!(f, "{name}"),
            Atom::Literal(lit) => match lit {
                Literal::Int(i) => write!(f, "{i}"),
                Literal::UInt(u) => write!(f, "{u}"),
                Literal::Float(fl) => write!(f, "{fl}"),
                Literal::String(s) => write!(f, "\"{s}\""),
                Literal::Bool(b) => write!(f, "{b}"),
                Literal::Unit => write!(f, "()"),
            },
        }
    }
}

impl fmt::Display for AnfExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnfExpr::Atom(a) => write!(f, "{a}"),
            AnfExpr::Binary { op, lhs, rhs } => write!(f, "{lhs} {op:?} {rhs}"),
            AnfExpr::Unary { op, operand } => write!(f, "{op:?} {operand}"),
            AnfExpr::Call { callee, args } => {
                write!(f, "{callee}(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ")")
            }
            AnfExpr::MethodCall {
                receiver,
                method,
                args,
            } => {
                write!(f, "{receiver}.{method}(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ")")
            }
            AnfExpr::FieldAccess { receiver, field } => write!(f, "{receiver}.{field}"),
            AnfExpr::TupleAccess { receiver, index } => write!(f, "{receiver}.{index}"),
            AnfExpr::Index { receiver, index } => write!(f, "{receiver}[{index}]"),
            AnfExpr::Record { fields } => {
                write!(f, "{{ ")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, " }}")
            }
            AnfExpr::Array { elements } => {
                write!(f, "[")?;
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{el}")?;
                }
                write!(f, "]")
            }
            AnfExpr::Tuple { elements } => {
                write!(f, "(")?;
                for (i, el) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{el}")?;
                }
                write!(f, ")")
            }
            AnfExpr::Variant {
                type_name,
                variant,
                args,
            } => {
                if let Some(t) = type_name {
                    write!(f, "{t}.{variant}")?;
                } else {
                    write!(f, "{variant}")?;
                }
                if !args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{arg}")?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            AnfExpr::Closure { params, .. } => {
                write!(f, "(|")?;
                for (i, (p, _)) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{p}")?;
                }
                write!(f, "| => ...)")
            }
            AnfExpr::MakeClosure { fn_name, env } => {
                if let Some(e) = env {
                    write!(f, "make_closure({fn_name}, {e})")
                } else {
                    write!(f, "make_closure({fn_name})")
                }
            }
            AnfExpr::CallClosure { closure, args } => {
                write!(f, "call_closure({closure}")?;
                for arg in args {
                    write!(f, ", {arg}")?;
                }
                write!(f, ")")
            }
            AnfExpr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                write!(
                    f,
                    "if ({cond}) {{ {} }} else {{ {} }}",
                    then_branch.tail, else_branch.tail
                )
            }
            AnfExpr::Match { scrutinee, arms } => {
                write!(f, "match {scrutinee} {{ {} arms }}", arms.len())
            }
            AnfExpr::ReuseRecord { base, fields } => {
                write!(f, "reuse_record({base}, {{ ")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                write!(f, " }})")
            }
            AnfExpr::IsUnique(atom) => write!(f, "is_unique({atom})"),
        }
    }
}

impl fmt::Display for AnfStmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnfStmt::Let { var, ty, value, .. } => write!(f, "let {var}: {ty} = {value};"),
            AnfStmt::Expr(expr) => write!(f, "{expr};"),
            AnfStmt::IncRef { var } => write!(f, "inc_ref({var});"),
            AnfStmt::DecRef { var } => write!(f, "dec_ref({var});"),
            AnfStmt::SetField {
                receiver,
                field,
                value,
            } => write!(f, "{receiver}.{field} = {value};"),
        }
    }
}

impl fmt::Display for AnfTail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnfTail::Return(Some(a)) => write!(f, "return {a};"),
            AnfTail::Return(None) => write!(f, "return;"),
            AnfTail::TailCall { callee, args } => {
                write!(f, "tailcall {callee}(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{arg}")?;
                }
                write!(f, ");")
            }
            AnfTail::If {
                cond,
                then_branch,
                else_branch,
            } => {
                writeln!(f, "if ({cond}) {{")?;
                write!(f, "{then_branch}")?;
                if let Some(eb) = else_branch {
                    writeln!(f, "}} else {{")?;
                    write!(f, "{eb}")?;
                }
                write!(f, "}}")
            }
            AnfTail::Match { scrutinee, arms } => {
                writeln!(f, "match {scrutinee} {{")?;
                for arm in arms {
                    writeln!(f, "  => {{")?;
                    write!(f, "{}", arm.body)?;
                    writeln!(f, "  }}")?;
                }
                write!(f, "}}")
            }
            AnfTail::Atom(a) => write!(f, "{a}"),
        }
    }
}

impl fmt::Display for AnfBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for stmt in &self.stmts {
            writeln!(f, "  {stmt}")?;
        }
        writeln!(f, "  {}", self.tail)
    }
}

impl fmt::Display for AnfFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "function {}(", self.name)?;
        for (i, (p, ty)) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{p}: {ty}")?;
        }
        writeln!(f, "): {} {{", self.return_type)?;
        write!(f, "{}", self.body)?;
        writeln!(f, "}}")
    }
}

impl fmt::Display for AnfProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for func in &self.functions {
            writeln!(f, "{func}\n")?;
        }
        Ok(())
    }
}
