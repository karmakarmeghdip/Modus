//! Module interface extraction and cross-module import integration.

use crate::ast::{Declaration, ExportDecl, Program};
use crate::modules::graph::ModuleId;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::scope::{ConstructorInfo, Environment, FunctionSig, TraitDef, TypeDefInfo};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleInterface {
    pub module_id: ModuleId,
    pub exported_functions: HashMap<String, FunctionSig>,
    pub exported_types: HashMap<String, TypeDefInfo>,
    pub exported_constructors: HashMap<String, ConstructorInfo>,
    pub exported_traits: HashMap<String, TraitDef>,
    pub library_path: Option<PathBuf>,
    pub interface_hash: u64,
}

impl ModuleInterface {
    /// Extracts the public module interface from a typechecked program and its environment.
    pub fn extract(
        module_id: ModuleId,
        program: &Program,
        env: &Environment,
        library_path: Option<PathBuf>,
    ) -> Result<Self, TypeError> {
        let mut exported_functions = HashMap::new();
        let mut exported_types = HashMap::new();
        let mut exported_constructors = HashMap::new();
        let mut exported_traits = HashMap::new();

        let module_ident = module_id.module_ident();

        // 1. Process inline-exported declarations
        for decl in &program.declarations {
            if decl.node.is_exported() {
                match &decl.node {
                    Declaration::Function(f) => {
                        if let Some(sig) = env.lookup_function(&f.name) {
                            let mut exp_sig = sig.clone();
                            let mangled =
                                crate::modules::mangling::mangle_symbol(&module_ident, &f.name);
                            exp_sig.symbol_name = Some(mangled);
                            exported_functions.insert(f.name.clone(), exp_sig);
                        }
                    }
                    Declaration::Type(t) => {
                        if let Some(info) = env.types.get(&t.name) {
                            exported_types.insert(t.name.clone(), info.clone());
                            // Also export variant constructors if Union
                            if let TypeDefInfo::Union { variants, .. } = info {
                                for v in variants {
                                    let qualified = format!("{}.{}", t.name, v.name);
                                    if let Some(ctor) = env.constructors.get(&qualified) {
                                        exported_constructors
                                            .insert(qualified.clone(), ctor.clone());
                                        exported_constructors.insert(v.name.clone(), ctor.clone());
                                    }
                                }
                            }
                        }
                    }
                    Declaration::Trait(tr) => {
                        if let Some(tr_def) = env.lookup_trait(&tr.name) {
                            exported_traits.insert(tr.name.clone(), tr_def.clone());
                        }
                    }
                    Declaration::Impl(_) => {}
                    Declaration::Extern(ext) => {
                        for f in &ext.functions {
                            if f.node.is_exported
                                && let Some(sig) = env.lookup_function(&f.node.name)
                            {
                                exported_functions.insert(f.node.name.clone(), sig.clone());
                            }
                        }
                    }
                }
            }
        }

        // 2. Process export clauses (e.g. export { a, b as c };)
        for export_decl in &program.exports {
            if let ExportDecl::Named {
                specifiers,
                source: None,
            } = &export_decl.node
            {
                for spec in specifiers {
                    let local_name = &spec.name;
                    let export_name = spec.alias.as_ref().unwrap_or(local_name);

                    let mut found = false;
                    if let Some(sig) = env.lookup_function(local_name) {
                        let mut exported_sig = sig.clone();
                        exported_sig.name = export_name.clone();
                        if !exported_sig.is_c_abi {
                            let mangled =
                                crate::modules::mangling::mangle_symbol(&module_ident, export_name);
                            exported_sig.symbol_name = Some(mangled);
                        }
                        exported_functions.insert(export_name.clone(), exported_sig);
                        found = true;
                    }
                    if let Some(info) = env.types.get(local_name) {
                        exported_types.insert(export_name.clone(), info.clone());
                        found = true;
                    }
                    if let Some(tr) = env.lookup_trait(local_name) {
                        exported_traits.insert(export_name.clone(), tr.clone());
                        found = true;
                    }

                    if !found {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Cannot export undeclared symbol '{local_name}'"
                            )),
                            Some(export_decl.span),
                        ));
                    }
                }
            }
        }

        // 3. Compute deterministic interface hash
        let interface_hash = Self::compute_hash(
            &exported_functions,
            &exported_types,
            &exported_traits,
            &library_path,
        );

        Ok(Self {
            module_id,
            exported_functions,
            exported_types,
            exported_constructors,
            exported_traits,
            library_path,
            interface_hash,
        })
    }

    /// Injects exported symbols into a target environment according to an import declaration.
    pub fn import_into(
        &self,
        env: &mut Environment,
        clause: &crate::ast::ImportClause,
        span: crate::ast::Span,
    ) -> Result<(), TypeError> {
        match clause {
            crate::ast::ImportClause::Named(specifiers) => {
                for spec in specifiers {
                    let name = &spec.name;
                    let target_name = spec.alias.as_ref().unwrap_or(name);

                    let mut found = false;

                    if let Some(sig) = self.exported_functions.get(name) {
                        let mut imported_sig = sig.clone();
                        imported_sig.name = target_name.clone();
                        if self.library_path.is_some() {
                            imported_sig.is_c_abi = true;
                        }
                        let _ = env.define_function(imported_sig);
                        found = true;
                    }

                    if let Some(type_info) = self.exported_types.get(name) {
                        env.types.insert(target_name.clone(), type_info.clone());
                        // If union, import constructors
                        if let TypeDefInfo::Union { variants, .. } = type_info {
                            for v in variants {
                                let qualified = format!("{target_name}.{}", v.name);
                                if let Some(ctor) = self
                                    .exported_constructors
                                    .get(&format!("{name}.{}", v.name))
                                {
                                    env.constructors.insert(qualified, ctor.clone());
                                    env.constructors.insert(v.name.clone(), ctor.clone());
                                }
                            }
                        }
                        found = true;
                    }

                    if let Some(trait_def) = self.exported_traits.get(name) {
                        let mut imported_trait = trait_def.clone();
                        imported_trait.name = target_name.clone();
                        let _ = env.define_trait(imported_trait, span);
                        found = true;
                    }

                    if !found {
                        return Err(TypeError::new(
                            TypeErrorKind::General(format!(
                                "Symbol '{name}' is not exported by module '{}'",
                                self.module_id
                            )),
                            Some(span),
                        ));
                    }
                }
            }

            crate::ast::ImportClause::Namespace(alias) => {
                // Register namespace-qualified functions
                for (fn_name, sig) in &self.exported_functions {
                    let qualified = format!("{alias}.{fn_name}");
                    let mut imported_sig = sig.clone();
                    imported_sig.name = qualified.clone();

                    for (_, p_ty) in &mut imported_sig.params {
                        *p_ty = qualify_type(p_ty, alias, &self.exported_types);
                    }
                    imported_sig.return_type =
                        qualify_type(&imported_sig.return_type, alias, &self.exported_types);

                    if self.library_path.is_some() {
                        imported_sig.is_c_abi = true;
                    }

                    let _ = env.define_function(imported_sig.clone());

                    let qualified_under = format!("{alias}_{fn_name}");
                    imported_sig.name = qualified_under;
                    let _ = env.define_function(imported_sig);
                }

                // Register namespace-qualified types
                for (type_name, type_info) in &self.exported_types {
                    let qualified = format!("{alias}.{type_name}");
                    env.types.insert(qualified.clone(), type_info.clone());
                    if let TypeDefInfo::Union { variants, .. } = type_info {
                        for v in variants {
                            let ctor_qualified = format!("{qualified}.{}", v.name);
                            if let Some(ctor) = self
                                .exported_constructors
                                .get(&format!("{type_name}.{}", v.name))
                            {
                                env.constructors.insert(ctor_qualified, ctor.clone());
                            }
                        }
                    }
                }

                // Register namespace-qualified traits
                for (trait_name, trait_def) in &self.exported_traits {
                    let qualified = format!("{alias}.{trait_name}");
                    let mut imported_trait = trait_def.clone();
                    imported_trait.name = qualified;
                    let _ = env.define_trait(imported_trait, span);
                }
            }

            crate::ast::ImportClause::SideEffect => {
                // Side-effect only import: no symbols imported into lexical scope
            }
        }

        Ok(())
    }

    fn compute_hash(
        functions: &HashMap<String, FunctionSig>,
        types: &HashMap<String, TypeDefInfo>,
        traits: &HashMap<String, TraitDef>,
        library_path: &Option<PathBuf>,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();

        // Sort function signatures
        let mut fn_keys: Vec<_> = functions.keys().collect();
        fn_keys.sort();
        for k in fn_keys {
            let sig = &functions[k];
            k.hash(&mut hasher);
            sig.params.len().hash(&mut hasher);
            for (p_name, p_ty) in &sig.params {
                p_name.hash(&mut hasher);
                format!("{p_ty:?}").hash(&mut hasher);
            }
            format!("{:?}", sig.return_type).hash(&mut hasher);
            sig.is_effectful.hash(&mut hasher);
        }

        // Sort types
        let mut type_keys: Vec<_> = types.keys().collect();
        type_keys.sort();
        for k in type_keys {
            k.hash(&mut hasher);
            format!("{:?}", types[k]).hash(&mut hasher);
        }

        // Sort traits
        let mut trait_keys: Vec<_> = traits.keys().collect();
        trait_keys.sort();
        for k in trait_keys {
            k.hash(&mut hasher);
            format!("{:?}", traits[k]).hash(&mut hasher);
        }

        if let Some(p) = library_path {
            p.hash(&mut hasher);
        }

        hasher.finish()
    }
}

fn qualify_type(
    ty: &crate::typechecker::types::Type,
    alias: &str,
    exported_types: &HashMap<String, TypeDefInfo>,
) -> crate::typechecker::types::Type {
    use crate::typechecker::types::Type;
    match ty {
        Type::Named { name, args } => {
            let new_name = if exported_types.contains_key(name) {
                format!("{alias}.{name}")
            } else {
                name.clone()
            };
            let new_args = args
                .iter()
                .map(|a| qualify_type(a, alias, exported_types))
                .collect();
            Type::Named {
                name: new_name,
                args: new_args,
            }
        }
        Type::Array(inner) => Type::Array(Box::new(qualify_type(inner, alias, exported_types))),
        Type::Function { params, ret } => Type::Function {
            params: params
                .iter()
                .map(|p| qualify_type(p, alias, exported_types))
                .collect(),
            ret: Box::new(qualify_type(ret, alias, exported_types)),
        },
        Type::Record(fields) => {
            let new_fields = fields
                .iter()
                .map(|(k, v)| (k.clone(), qualify_type(v, alias, exported_types)))
                .collect();
            Type::Record(new_fields)
        }
        Type::Tuple(elems) => {
            let new_elems = elems
                .iter()
                .map(|e| qualify_type(e, alias, exported_types))
                .collect();
            Type::Tuple(new_elems)
        }
        _ => ty.clone(),
    }
}
