use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

#[derive(Clone, Debug)]
enum BindingKind {
    Import(String),
    Shadow,
}

#[derive(Clone, Debug)]
struct Binding {
    position: usize,
    kind: BindingKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScopeKind {
    Module,
    Function,
    Class,
    Lambda,
    Comprehension,
}

#[derive(Debug)]
struct Scope {
    kind: ScopeKind,
    start: usize,
    end: usize,
    parent: Option<usize>,
    bindings: HashMap<String, Vec<Binding>>,
    globals: HashSet<String>,
    nonlocals: HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum NameResolution {
    Canonical(String),
    ShadowedImport,
    Unresolved,
}

pub struct NameResolver {
    scopes: Vec<Scope>,
}

impl NameResolver {
    pub fn new(root: Node, source: &[u8]) -> Self {
        let mut resolver = Self {
            scopes: vec![Scope {
                kind: ScopeKind::Module,
                start: root.start_byte(),
                end: root.end_byte(),
                parent: None,
                bindings: HashMap::new(),
                globals: HashSet::new(),
                nonlocals: HashSet::new(),
            }],
        };
        resolver.collect(root, source, 0);
        resolver
    }

    pub fn resolve(&self, node: Node, name: &str) -> NameResolution {
        let Some((root, suffix)) = split_name(name) else {
            return NameResolution::Unresolved;
        };
        let scope = self.innermost_scope(node.start_byte());
        match self.resolve_root(scope, root, node.start_byte()) {
            RootResolution::Import(module) => {
                NameResolution::Canonical(format!("{module}{suffix}"))
            }
            RootResolution::ShadowedImport => NameResolution::ShadowedImport,
            RootResolution::Unresolved => NameResolution::Unresolved,
        }
    }

    fn collect(&mut self, node: Node, source: &[u8], scope: usize) {
        match node.kind() {
            "function_definition" => {
                self.add_definition_binding(node, source, scope);
                self.collect_scoped_body(node, source, scope, ScopeKind::Function);
            }
            "class_definition" => {
                self.add_definition_binding(node, source, scope);
                self.collect_scoped_body(node, source, scope, ScopeKind::Class);
            }
            "lambda" => self.collect_scoped_body(node, source, scope, ScopeKind::Lambda),
            "list_comprehension"
            | "set_comprehension"
            | "dictionary_comprehension"
            | "generator_expression" => {
                self.collect_scoped_node(node, source, scope, ScopeKind::Comprehension);
            }
            _ => {
                self.collect_binding(node, source, scope);
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    self.collect(child, source, scope);
                }
            }
        }
    }

    fn collect_scoped_body(&mut self, node: Node, source: &[u8], parent: usize, kind: ScopeKind) {
        let Some(body) = node.child_by_field_name("body") else {
            return;
        };
        let child_scope = self.scopes.len();
        self.scopes.push(Scope {
            kind,
            start: body.start_byte(),
            end: body.end_byte(),
            parent: Some(parent),
            bindings: HashMap::new(),
            globals: HashSet::new(),
            nonlocals: HashSet::new(),
        });

        if let Some(parameters) = node.child_by_field_name("parameters") {
            self.collect_parameters(parameters, source, child_scope);
        }

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.id() == body.id() {
                self.collect(child, source, child_scope);
            } else {
                self.collect(child, source, parent);
            }
        }
    }

    fn collect_scoped_node(&mut self, node: Node, source: &[u8], parent: usize, kind: ScopeKind) {
        let child_scope = self.scopes.len();
        self.scopes.push(Scope {
            kind,
            start: node.start_byte(),
            end: node.end_byte(),
            parent: Some(parent),
            bindings: HashMap::new(),
            globals: HashSet::new(),
            nonlocals: HashSet::new(),
        });

        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            self.collect(child, source, child_scope);
        }
    }

    fn collect_binding(&mut self, node: Node, source: &[u8], scope: usize) {
        match node.kind() {
            "import_statement" => self.collect_imports(node, source, scope, None),
            "import_from_statement" => {
                let module = node
                    .child_by_field_name("module_name")
                    .and_then(|module| module.utf8_text(source).ok());
                self.collect_imports(node, source, scope, module);
            }
            "assignment" | "augmented_assignment" | "for_statement" | "for_in_clause" => {
                if let Some(target) = node.child_by_field_name("left") {
                    self.add_target_bindings(target, source, scope, node.start_byte());
                }
            }
            "named_expression" => {
                if let Some(target) = node.child_by_field_name("name") {
                    self.add_target_bindings(target, source, scope, node.start_byte());
                }
            }
            "except_clause" => {
                if let Some(target) = node.child_by_field_name("alias") {
                    self.add_target_bindings(target, source, scope, node.start_byte());
                }
            }
            "as_pattern" => {
                if let Some(target) = node.child_by_field_name("alias") {
                    self.add_target_bindings(target, source, scope, node.start_byte());
                }
            }
            "global_statement" => {
                self.collect_declarations(node, source, scope, true);
            }
            "nonlocal_statement" => {
                self.collect_declarations(node, source, scope, false);
            }
            _ => {}
        }
    }

    fn collect_declarations(&mut self, node: Node, source: &[u8], scope: usize, global: bool) {
        let mut cursor = node.walk();
        for name in node.named_children(&mut cursor) {
            if let Ok(name) = name.utf8_text(source) {
                if global {
                    self.scopes[scope].globals.insert(name.to_owned());
                } else {
                    self.scopes[scope].nonlocals.insert(name.to_owned());
                }
            }
        }
    }

    fn collect_imports(&mut self, node: Node, source: &[u8], scope: usize, module: Option<&str>) {
        let mut cursor = node.walk();
        for imported in node.children_by_field_name("name", &mut cursor) {
            let (name, alias) = if imported.kind() == "aliased_import" {
                (
                    imported
                        .child_by_field_name("name")
                        .and_then(|name| name.utf8_text(source).ok()),
                    imported
                        .child_by_field_name("alias")
                        .and_then(|alias| alias.utf8_text(source).ok()),
                )
            } else {
                (imported.utf8_text(source).ok(), None)
            };
            let Some(name) = name else {
                continue;
            };

            let (binding_name, canonical) = match module {
                Some(module) => (
                    alias.unwrap_or_else(|| first_segment(name)),
                    format!("{module}.{name}"),
                ),
                None => match alias {
                    Some(alias) => (alias, name.to_owned()),
                    None => {
                        let root = first_segment(name);
                        (root, root.to_owned())
                    }
                },
            };
            self.add_binding(
                scope,
                binding_name,
                node.start_byte(),
                BindingKind::Import(canonical),
            );
        }
    }

    fn collect_parameters(&mut self, parameters: Node, source: &[u8], scope: usize) {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            let target = match parameter.kind() {
                "identifier" | "tuple_pattern" => Some(parameter),
                _ => parameter.child_by_field_name("name").or_else(|| {
                    matches!(
                        parameter.kind(),
                        "list_splat_pattern" | "dictionary_splat_pattern"
                    )
                    .then_some(parameter)
                }),
            };
            if let Some(target) = target {
                self.add_target_bindings(target, source, scope, 0);
            }
        }
    }

    fn add_definition_binding(&mut self, node: Node, source: &[u8], scope: usize) {
        if let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(source).ok())
        {
            self.add_binding(scope, name, node.start_byte(), BindingKind::Shadow);
        }
    }

    fn add_target_bindings(&mut self, target: Node, source: &[u8], scope: usize, position: usize) {
        match target.kind() {
            "identifier" | "keyword_identifier" => {
                if let Ok(name) = target.utf8_text(source) {
                    self.add_binding(scope, name, position, BindingKind::Shadow);
                }
            }
            "attribute" | "subscript" => {}
            _ => {
                let mut cursor = target.walk();
                for child in target.named_children(&mut cursor) {
                    self.add_target_bindings(child, source, scope, position);
                }
            }
        }
    }

    fn add_binding(&mut self, scope: usize, name: &str, position: usize, kind: BindingKind) {
        self.scopes[scope]
            .bindings
            .entry(name.to_owned())
            .or_default()
            .push(Binding { position, kind });
    }

    fn innermost_scope(&self, position: usize) -> usize {
        self.scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| scope.start <= position && position <= scope.end)
            .min_by_key(|(_, scope)| scope.end - scope.start)
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    fn resolve_root(&self, scope: usize, name: &str, position: usize) -> RootResolution {
        let current = &self.scopes[scope];
        if scope != 0 && current.globals.contains(name) {
            return self.resolve_root(0, name, position);
        }
        if current.nonlocals.contains(name) {
            return self.resolve_parent(current, name, position);
        }

        if let Some(bindings) = current.bindings.get(name) {
            let previous = bindings
                .iter()
                .filter(|binding| binding.position <= position)
                .max_by_key(|binding| binding.position);

            if let Some(binding) = previous {
                return match &binding.kind {
                    BindingKind::Import(module) => RootResolution::Import(module.clone()),
                    BindingKind::Shadow => {
                        if bindings.iter().any(|candidate| {
                            candidate.position < binding.position
                                && matches!(candidate.kind, BindingKind::Import(_))
                        }) || self.parent_resolves_import(current, name, position)
                        {
                            RootResolution::ShadowedImport
                        } else {
                            RootResolution::Unresolved
                        }
                    }
                };
            }

            if matches!(
                current.kind,
                ScopeKind::Function | ScopeKind::Lambda | ScopeKind::Comprehension
            ) {
                return if self.parent_resolves_import(current, name, position) {
                    RootResolution::ShadowedImport
                } else {
                    RootResolution::Unresolved
                };
            }
        }

        self.resolve_parent(current, name, position)
    }

    fn parent_resolves_import(&self, scope: &Scope, name: &str, position: usize) -> bool {
        matches!(
            self.resolve_parent(scope, name, position),
            RootResolution::Import(_) | RootResolution::ShadowedImport
        )
    }

    fn resolve_parent(&self, scope: &Scope, name: &str, position: usize) -> RootResolution {
        let mut parent = scope.parent;
        if matches!(
            scope.kind,
            ScopeKind::Function | ScopeKind::Lambda | ScopeKind::Comprehension
        ) {
            while parent.is_some_and(|index| self.scopes[index].kind == ScopeKind::Class) {
                parent = parent.and_then(|index| self.scopes[index].parent);
            }
        }
        parent
            .map(|parent| self.resolve_root(parent, name, position))
            .unwrap_or(RootResolution::Unresolved)
    }
}

enum RootResolution {
    Import(String),
    ShadowedImport,
    Unresolved,
}

fn split_name(name: &str) -> Option<(&str, &str)> {
    let root_end = name.find('.').unwrap_or(name.len());
    let root = &name[..root_end];
    (!root.is_empty()
        && root
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric()))
    .then_some((root, &name[root_end..]))
}

fn first_segment(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}
