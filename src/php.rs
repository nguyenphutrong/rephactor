use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Position, Range, TextEdit, Url, WorkspaceEdit,
};
use tree_sitter::{Node, Parser};

use crate::document::{byte_offset_for_lsp_position, lsp_position_for_byte_offset};

const ACTION_TITLE: &str = "Add names to arguments";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    parameters: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ClassInfo {
    methods: HashMap<String, Signature>,
    constructor: Option<Signature>,
    parents: Vec<String>,
    interfaces: Vec<String>,
    traits: Vec<String>,
}

#[derive(Debug, Default)]
struct ImportMap {
    classes: HashMap<String, String>,
}

#[derive(Debug, Default)]
struct SymbolIndex {
    functions: HashMap<String, Vec<Signature>>,
    classes: HashMap<String, ClassInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CallInfo {
    target: CallTarget,
    arguments: Vec<ArgumentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CallTarget {
    Function(String),
    StaticMethod { class_name: String, method: String },
    Constructor { class_name: String },
    InstanceMethod { variable: String, method: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArgumentInfo {
    insert_byte: usize,
    name: Option<String>,
    is_unpacking: bool,
}

fn named_argument_code_action_with_open_documents(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
) -> Option<CodeAction> {
    if !document_supports_named_arguments(uri) {
        return None;
    }

    let byte_offset = byte_offset_for_lsp_position(text, position)?;
    let tree = parse_php(text)?;
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let call = find_call_at_byte(root, text, byte_offset)?;
    let index = SymbolIndex::for_document_and_project(uri, text, open_documents);
    let signature = index.resolve(
        &call.target,
        root,
        text,
        byte_offset,
        namespace.as_deref(),
        &imports,
    )?;
    let edits = edits_for_call(text, &call, &signature)?;
    let title = action_title_for_edits(&edits);

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit::new(changes)),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

pub fn code_actions_for_position_with_open_documents(
    uri: &Url,
    text: &str,
    position: Position,
    open_documents: &HashMap<Url, String>,
) -> Vec<CodeActionOrCommand> {
    named_argument_code_action_with_open_documents(uri, text, position, open_documents)
        .map(CodeActionOrCommand::CodeAction)
        .into_iter()
        .collect()
}

fn parse_php(text: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    (!tree.root_node().has_error()).then_some(tree)
}

impl ImportMap {
    fn from_root(root: Node, text: &str) -> Self {
        let mut imports = Self::default();
        collect_imports(root, text, &mut imports);
        imports
    }

    fn insert_class(&mut self, alias: String, fqn: String) {
        self.classes
            .insert(normalize_symbol_key(&alias), clean_name_text(&fqn));
    }

    fn resolve_class_name(&self, name: &str, namespace: Option<&str>) -> Vec<String> {
        let name = clean_name_text(name);
        if name.starts_with('\\') {
            return vec![name.trim_start_matches('\\').to_string()];
        }

        let mut segments = name.split('\\');
        let first_segment = segments.next().unwrap_or_default();
        let rest = segments.collect::<Vec<_>>();

        if let Some(imported) = self.classes.get(&normalize_symbol_key(first_segment)) {
            let mut resolved = imported.clone();
            if !rest.is_empty() {
                resolved.push('\\');
                resolved.push_str(&rest.join("\\"));
            }
            return vec![resolved];
        }

        name_candidates(&name, namespace)
    }
}

impl SymbolIndex {
    fn for_document_and_project(
        uri: &Url,
        text: &str,
        open_documents: &HashMap<Url, String>,
    ) -> Self {
        let mut index = Self::default();

        if let Ok(document_path) = uri.to_file_path()
            && let Some(project_root) = find_project_root(&document_path)
        {
            let open_project_documents = open_project_documents(open_documents);
            index.index_project(&project_root, &open_project_documents);
        }

        index.index_text(text);
        index
    }

    fn index_project(&mut self, project_root: &Path, open_documents: &HashMap<PathBuf, String>) {
        let Some(paths) = composer_autoload_paths(project_root) else {
            return;
        };

        for path in paths {
            self.index_php_path(&path, open_documents);
        }
    }

    fn index_php_path(&mut self, path: &Path, open_documents: &HashMap<PathBuf, String>) {
        if path.is_dir() {
            self.index_php_files(path, open_documents);
            return;
        }

        if path.extension().and_then(|extension| extension.to_str()) != Some("php") {
            return;
        }

        if let Some(open_text) = open_documents.get(path) {
            self.index_text(open_text);
            return;
        }

        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        self.index_text(&text);
    }

    fn index_php_files(&mut self, root: &Path, open_documents: &HashMap<PathBuf, String>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                self.index_php_path(&path, open_documents);
                continue;
            }

            self.index_php_path(&path, open_documents);
        }
    }

    fn index_text(&mut self, text: &str) {
        let Some(tree) = parse_php(text) else {
            return;
        };

        index_children(self, tree.root_node(), text, None);
    }

    fn add_function(&mut self, fqn: String, signature: Signature) {
        let signatures = self
            .functions
            .entry(normalize_symbol_key(&fqn))
            .or_default();
        if !signatures.contains(&signature) {
            signatures.push(signature);
        }
    }

    fn add_class(&mut self, fqn: String, class_info: ClassInfo) {
        self.classes.insert(normalize_symbol_key(&fqn), class_info);
    }

    fn resolve(
        &self,
        target: &CallTarget,
        root: Node,
        text: &str,
        byte_offset: usize,
        namespace: Option<&str>,
        imports: &ImportMap,
    ) -> Option<Signature> {
        match target {
            CallTarget::Function(name) => self.resolve_function(name, namespace),
            CallTarget::StaticMethod { class_name, method } => self
                .resolve_class(class_name, namespace, imports)
                .and_then(|class_info| self.resolve_method(class_info, method)),
            CallTarget::Constructor { class_name } => self
                .resolve_class(class_name, namespace, imports)
                .and_then(|class_info| class_info.constructor.clone()),
            CallTarget::InstanceMethod { variable, method } => {
                let variable_types =
                    variable_types_at_byte(root, text, byte_offset, namespace, imports);
                let class_name = variable_types.get(variable)?;
                self.resolve_class(class_name, namespace, imports)
                    .and_then(|class_info| self.resolve_method(class_info, method))
            }
        }
    }

    fn resolve_function(&self, name: &str, namespace: Option<&str>) -> Option<Signature> {
        for candidate in name_candidates(name, namespace) {
            if let Some(signatures) = self.functions.get(&normalize_symbol_key(&candidate))
                && signatures.len() == 1
            {
                return signatures.first().cloned();
            }
        }

        internal_function_signature(name)
    }

    fn resolve_class(
        &self,
        class_name: &str,
        namespace: Option<&str>,
        imports: &ImportMap,
    ) -> Option<&ClassInfo> {
        for candidate in imports.resolve_class_name(class_name, namespace) {
            if let Some(class_info) = self.classes.get(&normalize_symbol_key(&candidate)) {
                return Some(class_info);
            }
        }

        None
    }

    fn resolve_method(&self, class_info: &ClassInfo, method: &str) -> Option<Signature> {
        let method_key = normalize_method_key(method);
        if let Some(signature) = class_info.methods.get(&method_key) {
            return Some(signature.clone());
        }

        let mut signatures = Vec::new();
        let mut visited = Vec::new();

        for related_name in class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .chain(class_info.traits.iter())
        {
            self.collect_related_method_signatures(
                related_name,
                &method_key,
                &mut visited,
                &mut signatures,
            );
        }

        (signatures.len() == 1).then(|| signatures.remove(0))
    }

    fn collect_related_method_signatures(
        &self,
        class_name: &str,
        method_key: &str,
        visited: &mut Vec<String>,
        signatures: &mut Vec<Signature>,
    ) {
        let class_key = normalize_symbol_key(class_name);
        if visited.contains(&class_key) {
            return;
        }
        visited.push(class_key.clone());

        let Some(class_info) = self.classes.get(&class_key) else {
            return;
        };

        if let Some(signature) = class_info.methods.get(method_key)
            && !signatures.contains(signature)
        {
            signatures.push(signature.clone());
        }

        for related_name in class_info
            .parents
            .iter()
            .chain(class_info.interfaces.iter())
            .chain(class_info.traits.iter())
        {
            self.collect_related_method_signatures(related_name, method_key, visited, signatures);
        }
    }
}

fn open_project_documents(open_documents: &HashMap<Url, String>) -> HashMap<PathBuf, String> {
    open_documents
        .iter()
        .filter_map(|(uri, text)| {
            let path = uri.to_file_path().ok()?;
            (path.extension().and_then(|extension| extension.to_str()) == Some("php"))
                .then(|| (path, text.clone()))
        })
        .collect()
}

fn collect_imports(node: Node, text: &str, imports: &mut ImportMap) {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if child.kind() == "namespace_use_declaration" {
            index_use_declaration(child, text, imports);
            continue;
        }

        if child.kind() == "class_declaration" || child.kind() == "function_definition" {
            continue;
        }

        collect_imports(child, text, imports);
    }
}

fn index_use_declaration(node: Node, text: &str, imports: &mut ImportMap) {
    let declaration_text = node_text(node, text).trim_start();
    if starts_with_use_kind(declaration_text, "function")
        || starts_with_use_kind(declaration_text, "const")
    {
        return;
    }

    if let Some(group) = direct_child_kind(node, "namespace_use_group") {
        let Some(prefix) = direct_child_kind(node, "namespace_name") else {
            return;
        };
        let prefix = clean_name_text(node_text(prefix, text));
        let mut cursor = group.walk();

        for clause in group
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "namespace_use_clause")
        {
            if let Some((alias, target)) = use_clause_names(clause, text) {
                imports.insert_class(alias, qualify_name(&target, Some(&prefix)));
            }
        }

        return;
    }

    let mut cursor = node.walk();
    for clause in node
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "namespace_use_clause")
    {
        if let Some((alias, target)) = use_clause_names(clause, text) {
            imports.insert_class(alias, target);
        }
    }
}

fn starts_with_use_kind(text: &str, kind: &str) -> bool {
    let Some(rest) = text.strip_prefix("use") else {
        return false;
    };

    rest.trim_start()
        .get(..kind.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(kind))
}

fn direct_child_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn use_clause_names(clause: Node, text: &str) -> Option<(String, String)> {
    let children = direct_name_children(clause);
    let target_node = children.first().copied()?;
    let target = clean_name_text(node_text(target_node, text));
    if target.is_empty() {
        return None;
    }

    let alias = if use_clause_has_alias(clause, text) {
        children
            .last()
            .copied()
            .filter(|node| node.kind() == "name")
            .map(|node| clean_name_text(node_text(node, text)))?
    } else {
        last_name_segment(&target).to_string()
    };

    Some((alias, target))
}

fn direct_name_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| is_name_node(*child))
        .collect()
}

fn use_clause_has_alias(clause: Node, text: &str) -> bool {
    node_text(clause, text)
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("as"))
}

fn index_children(index: &mut SymbolIndex, node: Node, text: &str, namespace: Option<String>) {
    let mut cursor = node.walk();
    let mut active_namespace = namespace;

    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                let namespace_name = child
                    .child_by_field_name("name")
                    .map(|name| clean_name_text(node_text(name, text)))
                    .filter(|name| !name.is_empty());

                if let Some(body) = child.child_by_field_name("body") {
                    index_children(index, body, text, namespace_name);
                } else {
                    active_namespace = namespace_name;
                }
            }
            "function_definition" => {
                index_function(index, child, text, active_namespace.as_deref());
            }
            "class_declaration" | "interface_declaration" | "trait_declaration" => {
                index_class(index, child, text, active_namespace.as_deref());
            }
            _ => index_children(index, child, text, active_namespace.clone()),
        }
    }
}

fn index_function(index: &mut SymbolIndex, node: Node, text: &str, namespace: Option<&str>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(parameters_node) = node.child_by_field_name("parameters") else {
        return;
    };

    let name = qualify_name(node_text(name_node, text), namespace);
    let signature = Signature {
        parameters: parameter_names(parameters_node, text),
    };
    index.add_function(name, signature);
}

fn index_class(index: &mut SymbolIndex, node: Node, text: &str, namespace: Option<&str>) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };

    let mut class_info = ClassInfo {
        parents: class_like_names_from_direct_child(node, "base_clause", text, namespace),
        interfaces: class_like_names_from_direct_child(
            node,
            "class_interface_clause",
            text,
            namespace,
        ),
        ..ClassInfo::default()
    };
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        if child.kind() == "use_declaration" {
            class_info
                .traits
                .extend(class_like_names(child, text, namespace));
            continue;
        }

        if child.kind() != "method_declaration" {
            continue;
        }

        index_method(&mut class_info, child, text);
    }

    index.add_class(
        qualify_name(node_text(name_node, text), namespace),
        class_info,
    );
}

fn index_method(class_info: &mut ClassInfo, node: Node, text: &str) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let Some(parameters_node) = node.child_by_field_name("parameters") else {
        return;
    };

    let method_name = node_text(name_node, text).to_string();
    let signature = Signature {
        parameters: parameter_names(parameters_node, text),
    };

    if method_name.eq_ignore_ascii_case("__construct") {
        class_info.constructor = Some(signature);
    } else {
        class_info
            .methods
            .insert(normalize_method_key(&method_name), signature);
    }
}

fn class_like_names_from_direct_child(
    node: Node,
    child_kind: &str,
    text: &str,
    namespace: Option<&str>,
) -> Vec<String> {
    direct_child_kind(node, child_kind)
        .map(|child| class_like_names(child, text, namespace))
        .unwrap_or_default()
}

fn class_like_names(node: Node, text: &str, namespace: Option<&str>) -> Vec<String> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| is_name_node(*child))
        .map(|child| qualify_name(node_text(child, text), namespace))
        .collect()
}

fn parameter_names(parameters_node: Node, text: &str) -> Vec<String> {
    let mut parameters = Vec::new();
    let mut cursor = parameters_node.walk();

    for child in parameters_node.named_children(&mut cursor) {
        if !matches!(
            child.kind(),
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter"
        ) {
            continue;
        }

        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };

        parameters.push(
            node_text(name_node, text)
                .trim_start_matches('$')
                .to_string(),
        );
    }

    parameters
}

fn namespace_at_byte(root: Node, text: &str, byte_offset: usize) -> Option<String> {
    let mut cursor = root.walk();
    let mut active_namespace = None;

    for child in root.named_children(&mut cursor) {
        if child.kind() != "namespace_definition" {
            continue;
        }

        if child.start_byte() > byte_offset {
            break;
        }

        let namespace_name = child
            .child_by_field_name("name")
            .map(|name| clean_name_text(node_text(name, text)))
            .filter(|name| !name.is_empty());

        if child.child_by_field_name("body").is_some() {
            if child.start_byte() <= byte_offset && byte_offset <= child.end_byte() {
                return namespace_name;
            }
        } else {
            active_namespace = namespace_name;
        }
    }

    active_namespace
}

fn find_call_at_byte(root: Node, text: &str, byte_offset: usize) -> Option<CallInfo> {
    find_smallest_call(root, text, byte_offset).and_then(|node| call_info(node, text))
}

fn find_smallest_call<'tree>(
    node: Node<'tree>,
    text: &str,
    byte_offset: usize,
) -> Option<Node<'tree>> {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return None;
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = find_smallest_call(child, text, byte_offset) {
            return Some(found);
        }
    }

    is_supported_call_kind(node.kind())
        .then_some(node)
        .filter(|call| call_info(*call, text).is_some())
}

fn is_supported_call_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_call_expression"
            | "scoped_call_expression"
            | "member_call_expression"
            | "object_creation_expression"
    )
}

fn call_info(node: Node, text: &str) -> Option<CallInfo> {
    let arguments_node = find_arguments_node(node)?;
    let arguments = argument_infos(arguments_node, text);

    if arguments.is_empty() {
        return None;
    }

    let target = match node.kind() {
        "function_call_expression" => {
            let function_node = node.child_by_field_name("function")?;
            if !is_name_node(function_node) {
                return None;
            }
            CallTarget::Function(clean_name_text(node_text(function_node, text)))
        }
        "scoped_call_expression" => {
            let scope_node = node.child_by_field_name("scope")?;
            if !is_name_node(scope_node) {
                return None;
            }
            let method = member_name_for_call(node, text)?;
            CallTarget::StaticMethod {
                class_name: clean_name_text(node_text(scope_node, text)),
                method,
            }
        }
        "member_call_expression" => {
            let object_node = node.child_by_field_name("object")?;
            if object_node.kind() != "variable_name" {
                return None;
            }
            let method = member_name_for_call(node, text)?;
            CallTarget::InstanceMethod {
                variable: node_text(object_node, text).to_string(),
                method,
            }
        }
        "object_creation_expression" => {
            let class_node = class_name_for_object_creation(node)?;
            if !is_name_node(class_node) {
                return None;
            }
            CallTarget::Constructor {
                class_name: clean_name_text(node_text(class_node, text)),
            }
        }
        _ => return None,
    };

    Some(CallInfo { target, arguments })
}

fn find_arguments_node(node: Node) -> Option<Node> {
    if let Some(arguments) = node.child_by_field_name("arguments") {
        return Some(arguments);
    }

    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "arguments")
}

fn class_name_for_object_creation(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| is_name_node(*child))
}

fn member_name_for_call(node: Node, text: &str) -> Option<String> {
    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        if child.kind() == "arguments" {
            continue;
        }
        if node.child_by_field_name("scope") == Some(child)
            || node.child_by_field_name("object") == Some(child)
        {
            continue;
        }
        if child.kind() == "name" {
            return Some(node_text(child, text).to_string());
        }
    }

    None
}

fn argument_infos(arguments_node: Node, text: &str) -> Vec<ArgumentInfo> {
    let mut arguments = Vec::new();
    let mut cursor = arguments_node.walk();

    for child in arguments_node.named_children(&mut cursor) {
        if child.kind() != "argument" {
            continue;
        }

        let argument_text = node_text(child, text);
        arguments.push(ArgumentInfo {
            insert_byte: child.start_byte(),
            name: named_argument_name(child, text),
            is_unpacking: argument_text.trim_start().starts_with("..."),
        });
    }

    arguments
}

fn named_argument_name(argument_node: Node, text: &str) -> Option<String> {
    let mut cursor = argument_node.walk();
    let mut children = argument_node.named_children(&mut cursor);
    let first_child = children.next()?;

    if first_child.kind() != "name" {
        return None;
    }

    let after_name = &text[first_child.end_byte()..argument_node.end_byte()];
    after_name
        .trim_start()
        .starts_with(':')
        .then(|| clean_name_text(node_text(first_child, text)))
}

fn edits_for_call(text: &str, call: &CallInfo, signature: &Signature) -> Option<Vec<TextEdit>> {
    if call.arguments.iter().any(|argument| argument.is_unpacking) {
        return None;
    }

    if call.arguments.len() > signature.parameters.len() || call.arguments.is_empty() {
        return None;
    }

    let mut edits = Vec::new();

    for (argument, parameter_name) in call.arguments.iter().zip(signature.parameters.iter()) {
        if let Some(argument_name) = &argument.name {
            if !argument_name.eq_ignore_ascii_case(parameter_name) {
                return None;
            }
            continue;
        }

        let position = lsp_position_for_byte_offset(text, argument.insert_byte)?;
        edits.push(TextEdit::new(
            Range {
                start: position,
                end: position,
            },
            format!("{parameter_name}: "),
        ));
    }

    (!edits.is_empty()).then_some(edits)
}

fn action_title_for_edits(edits: &[TextEdit]) -> String {
    if edits.len() == 1
        && let Some(parameter_name) = edits[0].new_text.strip_suffix(": ")
    {
        return format!("Add name identifier '{parameter_name}'");
    }

    ACTION_TITLE.to_string()
}

fn variable_types_at_byte(
    root: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
) -> HashMap<String, String> {
    let mut types = HashMap::new();
    collect_parameter_types(root, text, byte_offset, namespace, imports, &mut types);
    collect_assignment_types(root, text, byte_offset, namespace, imports, &mut types);
    types
}

fn collect_parameter_types(
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    if byte_offset < node.start_byte() || byte_offset > node.end_byte() {
        return;
    }

    if matches!(node.kind(), "function_definition" | "method_declaration")
        && let Some(parameters) = node.child_by_field_name("parameters")
    {
        let mut cursor = parameters.walk();
        for parameter in parameters.named_children(&mut cursor) {
            if parameter.kind() != "simple_parameter" {
                continue;
            }

            let Some(name_node) = parameter.child_by_field_name("name") else {
                continue;
            };
            let Some(type_node) = parameter.child_by_field_name("type") else {
                continue;
            };

            let type_name = first_named_type(type_node, text);
            if let Some(type_name) = type_name {
                types.insert(
                    node_text(name_node, text).to_string(),
                    qualify_type_name(&type_name, namespace, imports),
                );
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_parameter_types(child, text, byte_offset, namespace, imports, types);
    }
}

fn collect_assignment_types(
    node: Node,
    text: &str,
    byte_offset: usize,
    namespace: Option<&str>,
    imports: &ImportMap,
    types: &mut HashMap<String, String>,
) {
    if node.start_byte() >= byte_offset {
        return;
    }

    if node.kind() == "assignment_expression"
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && left.kind() == "variable_name"
        && right.kind() == "object_creation_expression"
        && let Some(class_node) = class_name_for_object_creation(right)
        && is_name_node(class_node)
    {
        types.insert(
            node_text(left, text).to_string(),
            qualify_type_name(
                &clean_name_text(node_text(class_node, text)),
                namespace,
                imports,
            ),
        );
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_assignment_types(child, text, byte_offset, namespace, imports, types);
    }
}

fn first_named_type(type_node: Node, text: &str) -> Option<String> {
    if matches!(
        type_node.kind(),
        "named_type" | "name" | "qualified_name" | "relative_name"
    ) {
        return Some(clean_name_text(node_text(type_node, text)));
    }

    let mut cursor = type_node.walk();
    for child in type_node.named_children(&mut cursor) {
        if let Some(type_name) = first_named_type(child, text) {
            return Some(type_name);
        }
    }

    None
}

fn find_project_root(document_path: &Path) -> Option<PathBuf> {
    let mut current = document_path.parent();

    while let Some(path) = current {
        if path.join("composer.json").is_file() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }

    None
}

fn document_supports_named_arguments(uri: &Url) -> bool {
    let Ok(document_path) = uri.to_file_path() else {
        return true;
    };
    let Some(project_root) = find_project_root(&document_path) else {
        return true;
    };

    project_supports_named_arguments(&project_root)
}

fn project_supports_named_arguments(project_root: &Path) -> bool {
    composer_php_constraint(project_root)
        .map(|constraint| php_constraint_requires_at_least_8(&constraint))
        .unwrap_or(true)
}

fn composer_php_constraint(project_root: &Path) -> Option<String> {
    let composer_text = fs::read_to_string(project_root.join("composer.json")).ok()?;
    let composer_json: serde_json::Value = serde_json::from_str(&composer_text).ok()?;
    composer_json
        .get("require")
        .and_then(|require| require.get("php"))
        .and_then(|php| php.as_str())
        .map(str::to_string)
}

fn php_constraint_requires_at_least_8(constraint: &str) -> bool {
    constraint
        .split("||")
        .map(str::trim)
        .filter(|alternative| !alternative.is_empty())
        .all(php_constraint_alternative_requires_at_least_8)
}

fn php_constraint_alternative_requires_at_least_8(alternative: &str) -> bool {
    alternative
        .split([',', ' '])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .any(php_constraint_token_requires_at_least_8)
}

fn php_constraint_token_requires_at_least_8(token: &str) -> bool {
    let token = token.trim_start_matches('=');
    let token = token.strip_prefix(">=").unwrap_or(token);
    let token = token.strip_prefix('^').unwrap_or(token);
    let token = token.strip_prefix('~').unwrap_or(token);
    let token = token.strip_prefix('v').unwrap_or(token);

    token == "8" || token.starts_with("8.") || token.starts_with("8.*") || token.starts_with("9")
}

fn composer_autoload_paths(project_root: &Path) -> Option<Vec<PathBuf>> {
    let composer_text = fs::read_to_string(project_root.join("composer.json")).ok()?;
    let composer_json: serde_json::Value = serde_json::from_str(&composer_text).ok()?;
    let autoload = composer_json.get("autoload")?;
    let mut roots = Vec::new();

    if let Some(psr4) = autoload.get("psr-4").and_then(|psr4| psr4.as_object()) {
        for value in psr4.values() {
            collect_composer_paths(project_root, value, &mut roots);
        }
    }

    if let Some(classmap) = autoload.get("classmap") {
        collect_composer_paths(project_root, classmap, &mut roots);
    }

    (!roots.is_empty()).then_some(roots)
}

fn collect_composer_paths(
    project_root: &Path,
    value: &serde_json::Value,
    paths: &mut Vec<PathBuf>,
) {
    if let Some(path) = value.as_str() {
        paths.push(project_root.join(path));
    } else if let Some(values) = value.as_array() {
        for value in values {
            collect_composer_paths(project_root, value, paths);
        }
    }
}

fn is_name_node(node: Node) -> bool {
    matches!(node.kind(), "name" | "qualified_name" | "relative_name")
}

fn node_text<'a>(node: Node, text: &'a str) -> &'a str {
    node.utf8_text(text.as_bytes()).unwrap_or("")
}

fn clean_name_text(name: &str) -> String {
    name.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn qualify_name(name: &str, namespace: Option<&str>) -> String {
    let name = clean_name_text(name);
    if name.starts_with('\\') || name.contains('\\') || namespace.unwrap_or("").is_empty() {
        name.trim_start_matches('\\').to_string()
    } else {
        format!("{}\\{}", namespace.unwrap_or(""), name)
    }
}

fn qualify_type_name(name: &str, namespace: Option<&str>, imports: &ImportMap) -> String {
    imports
        .resolve_class_name(name, namespace)
        .into_iter()
        .next()
        .unwrap_or_else(|| qualify_name(name, namespace))
}

fn name_candidates(name: &str, namespace: Option<&str>) -> Vec<String> {
    let name = clean_name_text(name);
    if name.starts_with('\\') {
        return vec![name.trim_start_matches('\\').to_string()];
    }
    if name.contains('\\') {
        return vec![qualify_name(&name, namespace)];
    }

    let mut candidates = Vec::new();
    if let Some(namespace) = namespace.filter(|namespace| !namespace.is_empty()) {
        candidates.push(format!("{namespace}\\{name}"));
    }
    candidates.push(name);
    candidates
}

fn last_name_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

fn normalize_symbol_key(name: &str) -> String {
    clean_name_text(name)
        .trim_start_matches('\\')
        .to_ascii_lowercase()
}

fn normalize_method_key(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn internal_function_signature(name: &str) -> Option<Signature> {
    let parameters = match normalize_symbol_key(name).as_str() {
        "array_filter" => &["array", "callback", "mode"][..],
        "array_key_exists" => &["key", "array"],
        "array_map" => &["callback", "array", "arrays"],
        "array_merge" => &["arrays"],
        "count" => &["value", "mode"],
        "explode" => &["separator", "string", "limit"],
        "implode" => &["separator", "array"],
        "in_array" => &["needle", "haystack", "strict"],
        "is_array" => &["value"],
        "json_decode" => &["json", "associative", "depth", "flags"],
        "json_encode" => &["value", "flags", "depth"],
        "preg_match" => &["pattern", "subject", "matches", "flags", "offset"],
        "str_contains" => &["haystack", "needle"],
        "str_replace" => &["search", "replace", "subject", "count"],
        "strlen" => &["string"],
        "trim" => &["string", "characters"],
        _ => return None,
    };

    Some(Signature {
        parameters: parameters
            .iter()
            .map(|parameter| parameter.to_string())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn uri() -> Url {
        Url::parse("file:///tmp/project/src/Example.php").expect("valid uri")
    }

    fn named_argument_code_action(uri: &Url, text: &str, position: Position) -> Option<CodeAction> {
        named_argument_code_action_with_open_documents(uri, text, position, &HashMap::new())
    }

    fn position(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn action_edits(text: &str, line: u32, character: u32) -> Vec<TextEdit> {
        let action = named_argument_code_action(&uri(), text, position(line, character))
            .expect("code action");
        action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&uri())
            .expect("edits")
    }

    fn apply_edits(text: &str, edits: &[TextEdit]) -> String {
        let mut output = text.to_string();
        let mut byte_edits = edits
            .iter()
            .map(|edit| {
                let byte_offset = byte_offset_for_lsp_position(&output, edit.range.start)
                    .expect("valid edit position");
                (byte_offset, edit.new_text.clone())
            })
            .collect::<Vec<_>>();

        byte_edits.sort_by_key(|(byte_offset, _)| *byte_offset);

        for (byte_offset, new_text) in byte_edits.into_iter().rev() {
            output.insert_str(byte_offset, &new_text);
        }

        output
    }

    fn unique_project_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("rephactor-test-{nanos}"))
    }

    #[test]
    fn converts_same_file_function_call() {
        let text =
            "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let edits = action_edits(text, 2, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_seeded_php_internal_function_call() {
        let text = "<?php\nstr_replace($search, $replace, $subject);\n";

        let edits = action_edits(text, 1, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nstr_replace(search: $search, replace: $replace, subject: $subject);\n"
        );
    }

    #[test]
    fn converts_namespaced_same_file_function_call() {
        let text = "<?php\nnamespace App\\Billing;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let edits = action_edits(text, 3, 5);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Billing;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call() {
        let text = "<?php\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 2, 20);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\InvoiceSender;\nnamespace App\\Models;\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 6, 20);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\InvoiceSender;\nnamespace App\\Models;\nclass InvoiceSender { public static function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_grouped_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints($shop_id, $promotion_id);\n";

        let edits = action_edits(text, 6, 35);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\{customer_supplier};\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\ncustomer_supplier::accumulatePoints(shop_id: $shop_id, promotion_id: $promotion_id);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_aliased_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models\\customer_supplier as CustomerSupplier;\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\nCustomerSupplier::accumulatePoints($shop_id, $promotion_id);\n";

        let edits = action_edits(text, 6, 35);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models\\customer_supplier as CustomerSupplier;\nnamespace App\\Models;\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id) {} }\nnamespace App\\Http;\nCustomerSupplier::accumulatePoints(shop_id: $shop_id, promotion_id: $promotion_id);\n"
        );
    }

    #[test]
    fn converts_static_method_call_through_imported_namespace_alias() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Models as Models;\nnamespace App\\Models;\nclass Customer { public static function sync($shop_id, $customer_id) {} }\nnamespace App\\Http;\nModels\\Customer::sync($shop_id, $customer_id);\n";

        let edits = action_edits(text, 6, 25);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Models as Models;\nnamespace App\\Models;\nclass Customer { public static function sync($shop_id, $customer_id) {} }\nnamespace App\\Http;\nModels\\Customer::sync(shop_id: $shop_id, customer_id: $customer_id);\n"
        );
    }

    #[test]
    fn converts_constructor_call() {
        let text = "<?php\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnew InvoiceJob($invoice, true);\n";

        let edits = action_edits(text, 2, 6);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnew InvoiceJob(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_constructor_call_through_import() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Jobs\\InvoiceJob;\nnamespace App\\Jobs;\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnamespace App\\Http;\nnew InvoiceJob($invoice, true);\n";

        let edits = action_edits(text, 6, 6);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Jobs\\InvoiceJob;\nnamespace App\\Jobs;\nclass InvoiceJob { public function __construct($invoice, $notify) {} }\nnamespace App\\Http;\nnew InvoiceJob(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_instance_method_when_variable_type_is_obvious() {
        let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        let edits = action_edits(text, 3, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\n$sender = new InvoiceSender();\n$sender->dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn converts_instance_method_from_imported_typed_parameter() {
        let text = "<?php\nnamespace App\\Http;\nuse App\\Services\\InvoiceSender;\nnamespace App\\Services;\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 7, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App\\Http;\nuse App\\Services\\InvoiceSender;\nnamespace App\\Services;\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nnamespace App\\Http;\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn converts_instance_method_from_typed_parameter() {
        let text = "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 3, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass InvoiceSender { public function dispatch($invoice, $notify) {} }\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn resolves_static_method_from_parent_class() {
        let text = "<?php\nclass BaseSender { public static function dispatch($invoice, $notify) {} }\nclass InvoiceSender extends BaseSender {}\nInvoiceSender::dispatch($invoice, true);\n";

        let edits = action_edits(text, 3, 25);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass BaseSender { public static function dispatch($invoice, $notify) {} }\nclass InvoiceSender extends BaseSender {}\nInvoiceSender::dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn resolves_instance_method_from_implemented_interface() {
        let text = "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements Sender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch($invoice, true);\n}\n";

        let edits = action_edits(text, 4, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\ninterface Sender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements Sender {}\nfunction run(InvoiceSender $sender, $invoice) {\n    $sender->dispatch(invoice: $invoice, notify: true);\n}\n"
        );
    }

    #[test]
    fn resolves_instance_method_from_used_trait() {
        let text = "<?php\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\nclass InvoiceSender { use Dispatchable; }\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        let edits = action_edits(text, 4, 15);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\nclass InvoiceSender { use Dispatchable; }\n$sender = new InvoiceSender();\n$sender->dispatch(invoice: $invoice, notify: true);\n"
        );
    }

    #[test]
    fn skips_inherited_method_when_signatures_conflict() {
        let text = "<?php\ninterface FirstSender { public function dispatch($invoice); }\ninterface SecondSender { public function dispatch($invoice, $notify); }\nclass InvoiceSender implements FirstSender, SecondSender {}\n$sender = new InvoiceSender();\n$sender->dispatch($invoice, true);\n";

        assert!(named_argument_code_action(&uri(), text, position(5, 15)).is_none());
    }

    #[test]
    fn resolves_project_functions_from_composer_psr4_roots() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");
        fs::write(
            src_dir.join("Functions.php"),
            "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\n",
        )
        .expect("write functions");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("file uri");
        let text = "<?php\nnamespace App;\nsend_invoice($invoice, true);\n";
        let action =
            named_argument_code_action(&caller_uri, text, position(2, 5)).expect("code action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App;\nsend_invoice(invoice: $invoice, notify: true);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn resolves_project_classes_from_composer_classmap_file() {
        let project_root = unique_project_root();
        let legacy_dir = project_root.join("legacy");
        let app_dir = project_root.join("app");
        fs::create_dir_all(&legacy_dir).expect("create legacy dir");
        fs::create_dir_all(&app_dir).expect("create app dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"classmap":["legacy/CustomerSupplier.php"]}}"#,
        )
        .expect("write composer");
        fs::write(
            legacy_dir.join("CustomerSupplier.php"),
            "<?php\nnamespace Legacy;\nclass CustomerSupplier { public static function sync($shop_id, $customer_id) {} }\n",
        )
        .expect("write classmap class");

        let caller_path = app_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("file uri");
        let text = "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync($shop_id, $customer_id);\n";
        let action =
            named_argument_code_action(&caller_uri, text, position(3, 25)).expect("code action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nnamespace App;\nuse Legacy\\CustomerSupplier;\nCustomerSupplier::sync(shop_id: $shop_id, customer_id: $customer_id);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn open_project_document_overrides_disk_symbols() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let service_path = src_dir.join("Service.php");
        fs::write(
            &service_path,
            "<?php\nnamespace App;\nclass Service { public static function sync($old) {} }\n",
        )
        .expect("write stale service");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let service_uri = Url::from_file_path(&service_path).expect("service uri");
        let caller_text = "<?php\nnamespace App;\nService::sync($first, $second);\n";
        let open_service_text = "<?php\nnamespace App;\nclass Service { public static function sync($first, $second) {} }\n";
        let open_documents = HashMap::from([(service_uri, open_service_text.to_string())]);

        assert!(named_argument_code_action(&caller_uri, caller_text, position(2, 10)).is_none());

        let action = named_argument_code_action_with_open_documents(
            &caller_uri,
            caller_text,
            position(2, 10),
            &open_documents,
        )
        .expect("code action from open service document");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(caller_text, &edits),
            "<?php\nnamespace App;\nService::sync(first: $first, second: $second);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn skips_project_when_composer_php_constraint_allows_php_7() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"require":{"php":"^7.4"},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let caller_text = "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        assert!(named_argument_code_action(&caller_uri, caller_text, position(3, 5)).is_none());

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn allows_project_when_composer_requires_php_8() {
        let project_root = unique_project_root();
        let src_dir = project_root.join("src");
        fs::create_dir_all(&src_dir).expect("create source dir");
        fs::write(
            project_root.join("composer.json"),
            r#"{"require":{"php":">=8.0 <9.0"},"autoload":{"psr-4":{"App\\":"src/"}}}"#,
        )
        .expect("write composer");

        let caller_path = src_dir.join("Caller.php");
        let caller_uri = Url::from_file_path(&caller_path).expect("caller uri");
        let caller_text = "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, true);\n";

        let action =
            named_argument_code_action(&caller_uri, caller_text, position(3, 5)).expect("action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&caller_uri)
            .expect("edits");

        assert_eq!(
            apply_edits(caller_text, &edits),
            "<?php\nnamespace App;\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n"
        );

        fs::remove_dir_all(project_root).expect("remove project root");
    }

    #[test]
    fn indexes_trait_and_interface_methods() {
        let mut index = SymbolIndex::default();
        index.index_text(
            "<?php\nnamespace App;\ntrait Dispatchable { public function dispatch($invoice, $notify) {} }\ninterface Sender { public function send($invoice, $notify); }\n",
        );

        let trait_info = index
            .classes
            .get(&normalize_symbol_key("App\\Dispatchable"))
            .expect("trait indexed");
        let interface_info = index
            .classes
            .get(&normalize_symbol_key("App\\Sender"))
            .expect("interface indexed");

        assert_eq!(
            trait_info
                .methods
                .get(&normalize_method_key("dispatch"))
                .expect("trait method")
                .parameters,
            vec!["invoice", "notify"]
        );
        assert_eq!(
            interface_info
                .methods
                .get(&normalize_method_key("send"))
                .expect("interface method")
                .parameters,
            vec!["invoice", "notify"]
        );
    }

    #[test]
    fn skips_calls_when_all_arguments_are_named() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(invoice: $invoice, notify: true);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 5)).is_none());
    }

    #[test]
    fn converts_missing_argument_names_in_partially_named_call() {
        let text = "<?php\nfunction send_invoice($invoice, $notify, $priority) {}\nsend_invoice(invoice: $invoice, notify: true, $priority);\n";

        let edits = action_edits(text, 2, 45);

        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nfunction send_invoice($invoice, $notify, $priority) {}\nsend_invoice(invoice: $invoice, notify: true, priority: $priority);\n"
        );
    }

    #[test]
    fn converts_single_missing_name_after_named_static_arguments() {
        let text = "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id, $customer_id, $is_update_transaction, $customer_used_point, $pay, $product, $multipay_methods, $order_id, $extra_discount, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    promotion_id: $order->promotion_id,\n    customer_id: $customer_id,\n    is_update_transaction: $is_update_transaction,\n    customer_used_point: $item['customer_used_point'] ?? 0,\n    pay: $request->pay,\n    product: $request->product,\n    multipay_methods: $multipay_methods,\n    order_id: $order->id,\n    extra_discount: $request->extra_discount,\n    grand_total: $request->grand_total,\n    $request->exchange_gift,\n);\n";

        let action = named_argument_code_action(&uri(), text, position(14, 5)).expect("action");
        let edits = action
            .edit
            .expect("workspace edit")
            .changes
            .expect("changes")
            .remove(&uri())
            .expect("edits");

        assert_eq!(action.title, "Add name identifier 'exchange_gift'");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].new_text, "exchange_gift: ");
        assert_eq!(
            apply_edits(text, &edits),
            "<?php\nclass customer_supplier { public static function accumulatePoints($shop_id, $promotion_id, $customer_id, $is_update_transaction, $customer_used_point, $pay, $product, $multipay_methods, $order_id, $extra_discount, $grand_total, $exchange_gift = null) {} }\ncustomer_supplier::accumulatePoints(\n    shop_id: $shop_id,\n    promotion_id: $order->promotion_id,\n    customer_id: $customer_id,\n    is_update_transaction: $is_update_transaction,\n    customer_used_point: $item['customer_used_point'] ?? 0,\n    pay: $request->pay,\n    product: $request->product,\n    multipay_methods: $multipay_methods,\n    order_id: $order->id,\n    extra_discount: $request->extra_discount,\n    grand_total: $request->grand_total,\n    exchange_gift: $request->exchange_gift,\n);\n"
        );
    }

    #[test]
    fn skips_partially_named_calls_when_existing_names_do_not_match_position() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice(notify: true, $invoice);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 25)).is_none());
    }

    #[test]
    fn skips_calls_with_unpacking() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\nsend_invoice($invoice, ...$flags);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 5)).is_none());
    }

    #[test]
    fn skips_dynamic_function_calls() {
        let text = "<?php\nfunction send_invoice($invoice, $notify) {}\n$fn($invoice, true);\n";

        assert!(named_argument_code_action(&uri(), text, position(2, 2)).is_none());
    }
}
