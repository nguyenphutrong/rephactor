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

pub fn named_argument_code_action(uri: &Url, text: &str, position: Position) -> Option<CodeAction> {
    let byte_offset = byte_offset_for_lsp_position(text, position)?;
    let tree = parse_php(text)?;
    let root = tree.root_node();
    let namespace = namespace_at_byte(root, text, byte_offset);
    let imports = ImportMap::from_root(root, text);
    let call = find_call_at_byte(root, text, byte_offset)?;
    let index = SymbolIndex::for_document_and_project(uri, text);
    let signature = index.resolve(
        &call.target,
        root,
        text,
        byte_offset,
        namespace.as_deref(),
        &imports,
    )?;
    let edits = edits_for_call(text, &call, &signature)?;

    let mut changes = HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(CodeAction {
        title: ACTION_TITLE.to_string(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        diagnostics: None,
        edit: Some(WorkspaceEdit::new(changes)),
        command: None,
        is_preferred: Some(true),
        disabled: None,
        data: None,
    })
}

pub fn code_actions_for_position(
    uri: &Url,
    text: &str,
    position: Position,
) -> Vec<CodeActionOrCommand> {
    named_argument_code_action(uri, text, position)
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
    fn for_document_and_project(uri: &Url, text: &str) -> Self {
        let mut index = Self::default();

        if let Ok(document_path) = uri.to_file_path()
            && let Some(project_root) = find_project_root(&document_path)
        {
            index.index_project(&project_root, Some((&document_path, text)));
        }

        index.index_text(text);
        index
    }

    fn index_project(&mut self, project_root: &Path, open_document: Option<(&Path, &str)>) {
        let Some(psr4_roots) = composer_psr4_roots(project_root) else {
            return;
        };

        for root in psr4_roots {
            self.index_php_files(&root, open_document);
        }
    }

    fn index_php_files(&mut self, root: &Path, open_document: Option<(&Path, &str)>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                self.index_php_files(&path, open_document);
                continue;
            }

            if path.extension().and_then(|extension| extension.to_str()) != Some("php") {
                continue;
            }

            if let Some((open_path, open_text)) = open_document
                && path == open_path
            {
                self.index_text(open_text);
                continue;
            }

            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            self.index_text(&text);
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
                .and_then(|class_info| {
                    class_info
                        .methods
                        .get(&normalize_method_key(method))
                        .cloned()
                }),
            CallTarget::Constructor { class_name } => self
                .resolve_class(class_name, namespace, imports)
                .and_then(|class_info| class_info.constructor.clone()),
            CallTarget::InstanceMethod { variable, method } => {
                let variable_types =
                    variable_types_at_byte(root, text, byte_offset, namespace, imports);
                let class_name = variable_types.get(variable)?;
                self.resolve_class(class_name, namespace, imports)
                    .and_then(|class_info| {
                        class_info
                            .methods
                            .get(&normalize_method_key(method))
                            .cloned()
                    })
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

        None
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
            "class_declaration" => {
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

    let mut class_info = ClassInfo::default();
    let mut cursor = body.walk();

    for child in body.named_children(&mut cursor) {
        if child.kind() != "method_declaration" {
            continue;
        }

        let Some(name_node) = child.child_by_field_name("name") else {
            continue;
        };
        let Some(parameters_node) = child.child_by_field_name("parameters") else {
            continue;
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

    index.add_class(
        qualify_name(node_text(name_node, text), namespace),
        class_info,
    );
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

fn composer_psr4_roots(project_root: &Path) -> Option<Vec<PathBuf>> {
    let composer_text = fs::read_to_string(project_root.join("composer.json")).ok()?;
    let composer_json: serde_json::Value = serde_json::from_str(&composer_text).ok()?;
    let psr4 = composer_json
        .get("autoload")
        .and_then(|autoload| autoload.get("psr-4"))?
        .as_object()?;
    let mut roots = Vec::new();

    for value in psr4.values() {
        if let Some(path) = value.as_str() {
            roots.push(project_root.join(path));
        } else if let Some(paths) = value.as_array() {
            for path in paths.iter().filter_map(|path| path.as_str()) {
                roots.push(project_root.join(path));
            }
        }
    }

    Some(roots)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn uri() -> Url {
        Url::parse("file:///tmp/project/src/Example.php").expect("valid uri")
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

        let edits = action_edits(text, 14, 5);

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
