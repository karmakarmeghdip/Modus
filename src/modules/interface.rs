//! Module interface extraction and cross-module import integration.

use crate::ast::{Declaration, ExportDecl, Program};
use crate::modules::graph::ModuleId;
use crate::modules::resolver::resolve_module_path;
use crate::typechecker::error::{TypeError, TypeErrorKind};
use crate::typechecker::scope::{
    ConstructorInfo, Environment, FunctionSig, ImplDef, TraitDef, TypeDefInfo,
};
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
    pub exported_impls: Vec<ImplDef>,
    pub library_path: Option<PathBuf>,
    pub interface_hash: u64,
}

impl ModuleInterface {
    /// Extracts the public module interface from a typechecked program and its environment.
    ///
    /// `dep_interfaces` must contain the interfaces of every module this one
    /// imports; it is used to merge `export { x } from "..."` re-exports into
    /// this module's interface (TypeScript-style barrel re-exports).
    pub fn extract(
        module_id: ModuleId,
        program: &Program,
        env: &Environment,
        library_path: Option<PathBuf>,
        dep_interfaces: &HashMap<ModuleId, ModuleInterface>,
    ) -> Result<Self, TypeError> {
        let mut exported_functions = HashMap::new();
        let mut exported_types = HashMap::new();
        let mut exported_constructors = HashMap::new();
        let mut exported_traits = HashMap::new();
        let mut exported_impls: Vec<ImplDef> = Vec::new();

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
                    Declaration::Impl(im) => {
                        // Find the typechecked impl in the environment and give
                        // its methods mangled cross-module symbols.
                        let target_type = env.resolve_ast_type(
                            &im.target_type.node,
                            &[],
                            Some(im.target_type.span),
                        )?;
                        if let Some(impl_def) = env.lookup_impls(&im.trait_name).and_then(|impls| {
                            impls.iter().find(|d| d.target_type == target_type).cloned()
                        }) {
                            exported_impls.push(Self::mangle_impl_symbols(impl_def, &module_ident));
                        }
                    }
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

        // 2. Process export clauses
        for export_decl in &program.exports {
            match &export_decl.node {
                ExportDecl::Named {
                    specifiers,
                    source: None,
                } => {
                    for spec in specifiers {
                        let local_name = &spec.name;
                        let export_name = spec.alias.as_ref().unwrap_or(local_name);

                        let mut found = false;
                        if let Some(sig) = env.lookup_function(local_name) {
                            let mut exported_sig = sig.clone();
                            exported_sig.name = export_name.clone();
                            if !exported_sig.is_c_abi {
                                let mangled = crate::modules::mangling::mangle_symbol(
                                    &module_ident,
                                    export_name,
                                );
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
                ExportDecl::Named {
                    specifiers,
                    source: Some(source),
                } => {
                    let dep =
                        Self::dep_interface(source, &module_id, dep_interfaces, export_decl.span)?;
                    for spec in specifiers {
                        let export_name = spec.alias.as_ref().unwrap_or(&spec.name);
                        Self::reexport_symbol(
                            &spec.name,
                            export_name,
                            dep,
                            &mut exported_functions,
                            &mut exported_types,
                            &mut exported_constructors,
                            &mut exported_traits,
                        )?;
                    }
                }
                ExportDecl::All {
                    alias: None,
                    source,
                } => {
                    let dep =
                        Self::dep_interface(source, &module_id, dep_interfaces, export_decl.span)?;
                    for (name, sig) in &dep.exported_functions {
                        exported_functions.entry(name.clone()).or_insert_with(|| {
                            let mut s = sig.clone();
                            s.name = name.clone();
                            s
                        });
                    }
                    for (name, info) in &dep.exported_types {
                        exported_types
                            .entry(name.clone())
                            .or_insert_with(|| info.clone());
                        if let TypeDefInfo::Union { variants, .. } = info {
                            for v in variants {
                                let qualified = format!("{name}.{}", v.name);
                                if let Some(ctor) = dep.exported_constructors.get(&qualified) {
                                    exported_constructors
                                        .entry(qualified)
                                        .or_insert_with(|| ctor.clone());
                                    exported_constructors
                                        .entry(v.name.clone())
                                        .or_insert_with(|| ctor.clone());
                                }
                            }
                        }
                    }
                    for (name, tr) in &dep.exported_traits {
                        exported_traits
                            .entry(name.clone())
                            .or_insert_with(|| tr.clone());
                    }
                    for impl_def in &dep.exported_impls {
                        if !exported_impls.iter().any(|d| {
                            d.trait_name == impl_def.trait_name
                                && d.target_type == impl_def.target_type
                        }) {
                            exported_impls.push(impl_def.clone());
                        }
                    }
                }
                // `export * as ns` re-exports: link-only in v1 (the graph
                // dependency is still registered); no flat symbols are merged.
                ExportDecl::All {
                    alias: Some(_),
                    source: _,
                } => {}
                ExportDecl::Declaration(_) => {}
            }
        }

        // 3. Compute deterministic interface hash
        let interface_hash = Self::compute_hash(
            &exported_functions,
            &exported_types,
            &exported_traits,
            &exported_impls,
            &library_path,
        );

        Ok(Self {
            module_id,
            exported_functions,
            exported_types,
            exported_constructors,
            exported_traits,
            exported_impls,
            library_path,
            interface_hash,
        })
    }

    /// Resolves a re-export source to a dependency interface.
    fn dep_interface<'a>(
        source: &str,
        module_id: &ModuleId,
        dep_interfaces: &'a HashMap<ModuleId, ModuleInterface>,
        span: crate::ast::Span,
    ) -> Result<&'a ModuleInterface, TypeError> {
        let dep_path = resolve_module_path(source, Some(module_id.path()))
            .map_err(|e| TypeError::new(TypeErrorKind::General(e.to_string()), Some(span)))?;
        let dep_id = ModuleId::new(dep_path);
        dep_interfaces.get(&dep_id).ok_or_else(|| {
            TypeError::new(
                TypeErrorKind::General(format!("Missing dependency interface for '{dep_id}'")),
                Some(span),
            )
        })
    }

    /// Copies one named symbol from a dependency interface into this one.
    fn reexport_symbol(
        name: &str,
        export_name: &str,
        dep: &ModuleInterface,
        functions: &mut HashMap<String, FunctionSig>,
        types: &mut HashMap<String, TypeDefInfo>,
        constructors: &mut HashMap<String, ConstructorInfo>,
        traits: &mut HashMap<String, TraitDef>,
    ) -> Result<(), TypeError> {
        let mut found = false;
        if let Some(sig) = dep.exported_functions.get(name) {
            let mut s = sig.clone();
            s.name = export_name.to_string();
            functions.insert(export_name.to_string(), s);
            found = true;
        }
        if let Some(info) = dep.exported_types.get(name) {
            types.insert(export_name.to_string(), info.clone());
            if let TypeDefInfo::Union { variants, .. } = info {
                for v in variants {
                    let qualified = format!("{name}.{}", v.name);
                    if let Some(ctor) = dep.exported_constructors.get(&qualified) {
                        constructors.insert(format!("{export_name}.{}", v.name), ctor.clone());
                        constructors.insert(v.name.clone(), ctor.clone());
                    }
                }
            }
            found = true;
        }
        if let Some(tr) = dep.exported_traits.get(name) {
            let mut t = tr.clone();
            t.name = export_name.to_string();
            traits.insert(export_name.to_string(), t);
            found = true;
        }
        if !found {
            return Err(TypeError::new(
                TypeErrorKind::General(format!(
                    "Symbol '{name}' is not exported by module '{}'",
                    dep.module_id
                )),
                None,
            ));
        }
        Ok(())
    }

    /// Assigns mangled cross-module symbol names to an impl's methods:
    /// `_modus_M_{module}_{Trait}_{method}_{sanitized_target}`.
    fn mangle_impl_symbols(impl_def: ImplDef, module_ident: &str) -> ImplDef {
        let target_key =
            crate::modules::mangling::sanitize_ident(&impl_def.target_type.to_string());
        let trait_name = impl_def.trait_name.clone();
        let mut result = impl_def;
        for (method_name, sig) in result.methods.iter_mut() {
            let mangled = crate::modules::mangling::mangle_symbol(
                module_ident,
                &format!("{trait_name}_{method_name}_{target_key}"),
            );
            sig.symbol_name = Some(mangled);
        }
        result
    }

    /// Injects exported symbols into a target environment according to an import declaration.
    pub fn import_into(
        &self,
        env: &mut Environment,
        clause: &crate::ast::ImportClause,
        span: crate::ast::Span,
    ) -> Result<(), TypeError> {
        // Exported trait impls always apply when a module is imported (trait
        // impls are global to the trait/type pair, like Rust's coherence):
        // any import of this module makes its impls resolvable in the importer.
        for impl_def in &self.exported_impls {
            env.register_impl(impl_def.clone());
        }

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
        impls: &[ImplDef],
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

        // Sort impls
        let mut impls_sorted: Vec<_> = impls.iter().collect();
        impls_sorted.sort_by(|a, b| {
            (a.trait_name.to_string(), a.target_type.to_string())
                .cmp(&(b.trait_name.to_string(), b.target_type.to_string()))
        });
        for im in impls_sorted {
            im.trait_name.hash(&mut hasher);
            im.target_type.to_string().hash(&mut hasher);
            let mut method_names: Vec<_> = im.methods.keys().collect();
            method_names.sort();
            for m in method_names {
                m.hash(&mut hasher);
                let sig = &im.methods[m];
                format!("{:?}", sig.params).hash(&mut hasher);
                format!("{:?}", sig.return_type).hash(&mut hasher);
            }
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
