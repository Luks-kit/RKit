use crate::ast::{Expr, ExtendItem, Stmt, ToolMethod};
use crate::lexer::{Span, TokenType};
use crate::types::LKitType;
use crate::types::StructDef;
use crate::value::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct TypeError {
    pub message: String,
    pub span: Option<Span>,
}

impl TypeError {
    pub fn new(message: impl Into<String>) -> Self {
        TypeError {
            message: message.into(),
            span: None,
        }
    }
    pub fn with_span(message: impl Into<String>, span: Span) -> Self {
        TypeError {
            message: message.into(),
            span: Some(span),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MethodSig {
    pub params: Vec<LKitType>, // includes 'this' param
    pub ret: LKitType,
}

#[derive(Debug, Clone)]
pub struct ExtendDef {
    pub init_params: Option<Vec<LKitType>>, // None if no init defined
    pub has_dinit: bool,
    pub methods: HashMap<String, MethodSig>,
}

pub struct TypeChecker {
    scopes: Vec<HashMap<String, (LKitType, Option<String>)>>,
    functions: HashMap<String, LKitType>,
    pub structs: HashMap<String, StructDef>,
    pub extends: HashMap<String, ExtendDef>,
    current_return_type: Option<LKitType>,
    pub modules: HashMap<String, Vec<Stmt>>,
    pub module_exports: HashMap<String, HashMap<String, LKitType>>,
    pub tools: HashMap<String, Vec<ToolMethod>>, // tool name -> method sigs
    pub implementations: HashMap<String, HashSet<String>>, // type -> set of tools it implements
    pub errors: Vec<TypeError>,
    borrow_state: HashMap<String, (usize, bool)>,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            structs: HashMap::new(),
            extends: HashMap::new(),
            current_return_type: None,
            modules: HashMap::new(),
            module_exports: HashMap::new(),
            tools: HashMap::new(),
            implementations: HashMap::new(),
            errors: Vec::new(),
            borrow_state: HashMap::new(),
        }
    }

    pub fn register_pass(&mut self, stmts: &[Stmt]) {
        // Zero'th pass: register all structs
        for stmt in stmts {
            if let Stmt::Struct { name, fields } = stmt {
                let typed_fields = fields
                    .iter()
                    .filter_map(|(fname, ftype)| {
                        LKitType::from_str(ftype).map(|t| (fname.clone(), t))
                    })
                    .collect();
                self.structs.insert(
                    name.clone(),
                    StructDef {
                        name: name.clone(),
                        fields: typed_fields,
                    },
                );
            }
            // register tools
            if let Stmt::Tool { name, methods } = stmt {
                self.tools.insert(name.clone(), methods.clone());
            }
        }

        // First pass: register all function signatures and externs
        for stmt in stmts {
            if let Stmt::Function {
                name,
                params,
                return_type,
                ..
            } = stmt
            {
                let param_types = params
                    .iter()
                    .filter_map(|(_, ty)| LKitType::from_str(ty))
                    .collect();
                let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                self.functions.insert(
                    name.clone(),
                    LKitType::Function {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
            if let Stmt::Extern {
                name,
                params,
                return_type,
                ..
            } = stmt
            {
                let param_types = params
                    .iter()
                    .filter_map(|(_, ty)| LKitType::from_str(ty))
                    .collect();
                let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                self.functions.insert(
                    name.clone(),
                    LKitType::Function {
                        params: param_types,
                        ret: Box::new(ret),
                    },
                );
            }
        }

        // Second pass: register extend and extend-with blocks
        for stmt in stmts {
            if let Stmt::Extend { type_name, items } = stmt {
                let mut def = ExtendDef {
                    init_params: None,
                    has_dinit: false,
                    methods: HashMap::new(),
                };
                for item in items {
                    match item {
                        ExtendItem::Init { params, .. } => {
                            let param_types = params
                                .iter()
                                .filter_map(|(_, ty)| LKitType::from_str(ty))
                                .collect();
                            def.init_params = Some(param_types);
                        }
                        ExtendItem::Dinit { .. } => {
                            def.has_dinit = true;
                        }
                        ExtendItem::Method {
                            name,
                            params,
                            return_type,
                            ..
                        } => {
                            let param_types = params
                                .iter()
                                .filter_map(|(_, ty)| LKitType::from_str(ty))
                                .collect();
                            let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                            def.methods.insert(
                                name.clone(),
                                MethodSig {
                                    params: param_types,
                                    ret,
                                },
                            );
                        }
                    }
                }
                self.extends.insert(type_name.clone(), def);
            }

            if let Stmt::ExtendWith {
                type_name,
                tool_name,
                items,
            } = stmt
            {
                // validate tool exists
                if !self.tools.contains_key(tool_name) {
                    self.stmt_error(stmt, format!("Unknown tool '{}'", tool_name));
                    continue;
                }

                // validate all tool methods are implemented
                let tool_methods = self.tools.get(tool_name).cloned().unwrap();
                let implemented: HashSet<String> = items
                    .iter()
                    .filter_map(|item| match item {
                        ExtendItem::Method { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .collect();

                for tool_method in &tool_methods {
                    if !implemented.contains(&tool_method.name) {
                        self.stmt_error(
                            stmt,
                            format!(
                                "'{}' does not implement '{}' required by tool '{}'",
                                type_name, tool_method.name, tool_name
                            ),
                        );
                    }
                }

                // register implementation
                self.implementations
                    .entry(type_name.clone())
                    .or_insert_with(HashSet::new)
                    .insert(tool_name.clone());

                // merge methods into extend def
                let mut def = self.extends.get(type_name).cloned().unwrap_or(ExtendDef {
                    init_params: None,
                    has_dinit: false,
                    methods: HashMap::new(),
                });
                for item in items {
                    if let ExtendItem::Method {
                        name,
                        params,
                        return_type,
                        ..
                    } = item
                    {
                        let param_types = params
                            .iter()
                            .filter_map(|(_, ty)| LKitType::from_str(ty))
                            .collect();
                        let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                        def.methods.insert(
                            name.clone(),
                            MethodSig {
                                params: param_types,
                                ret,
                            },
                        );
                    }
                }
                self.extends.insert(type_name.clone(), def);
            }
        }
    }

    pub fn check(&mut self, stmts: &[Stmt]) {
        // check everything
        for stmt in stmts {
            self.check_stmt(stmt);
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (_, (ty, referent)) in &scope {
                if let Some(ref_name) = referent {
                    match ty {
                        LKitType::Ref(_) => {
                            if let Some(state) = self.borrow_state.get_mut(ref_name) {
                                if state.0 > 0 {
                                    state.0 -= 1;
                                }
                            }
                        }
                        LKitType::StrictRef(_) => {
                            if let Some(state) = self.borrow_state.get_mut(ref_name) {
                                state.1 = false;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    fn define(&mut self, name: &str, ty: LKitType) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), (ty, None));
    }

    fn define_ref(&mut self, name: &str, ty: LKitType, referent: String) {
        self.scopes
            .last_mut()
            .unwrap()
            .insert(name.to_string(), (ty, Some(referent)));
    }

    fn lookup(&self, name: &str) -> Option<&LKitType> {
        for scope in self.scopes.iter().rev() {
            if let Some((ty, _)) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    fn stmt_error(&mut self, _stmt: &Stmt, message: impl Into<String>) {
        self.errors.push(TypeError::new(message));
    }

    fn expr_error(&mut self, _expr: &Expr, message: impl Into<String>) {
        self.errors.push(TypeError::new(message));
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl {
                name,
                value_type,
                initializer,
            } => {
                let expected = match LKitType::from_str(value_type) {
                    Some(t) => t,
                    None => {
                        self.stmt_error(
                            stmt,
                            format!("Unknown type '{}' for variable '{}'", value_type, name),
                        );
                        return;
                    }
                };
                match self.check_expr(initializer) {
                    Some(actual) if actual != expected => {
                        if actual == LKitType::Int && expected == LKitType::Byte {
                        } else {
                            self.stmt_error(
                                stmt,
                                format!(
                                    "Type mismatch in '{}': expected {:?}, got {:?}",
                                    name, expected, actual
                                ),
                            );
                        }
                    }
                    _ => {}
                }
                // track referent for handle types
                let referent = match initializer {
                    Expr::Ref(inner) | Expr::StrictRef(inner) => match inner.as_ref() {
                        Expr::Variable(n) => Some(n.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                match referent {
                    Some(r) => self.define_ref(name, expected, r),
                    None => self.define(name, expected),
                }
            }
            Stmt::LetDecl { name, initializer } => match self.check_expr(initializer) {
                Some(ty) => {
                    let referent = match initializer {
                        Expr::Ref(inner) | Expr::StrictRef(inner) => match inner.as_ref() {
                            Expr::Variable(n) => Some(n.clone()),
                            _ => None,
                        },
                        _ => None,
                    };
                    match referent {
                        Some(r) => self.define_ref(name, ty, r),
                        None => self.define(name, ty),
                    }
                }
                None => self.stmt_error(stmt, format!("Cannot infer type for '{}'", name)),
            },

            Stmt::Function {
                name: _,
                params,
                return_type,
                body,
            } => {
                let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                self.current_return_type = Some(ret.clone());
                self.push_scope();
                for (param_name, param_type) in params {
                    if let Some(ty) = LKitType::from_str(param_type) {
                        self.define(param_name, ty);
                    } else {
                        self.stmt_error(
                            stmt,
                            format!("Unknown type '{}' for param '{}'", param_type, param_name),
                        );
                    }
                }
                for s in body {
                    self.check_stmt(s);
                }
                self.pop_scope();
                self.current_return_type = None;
            }

            Stmt::Return(expr) => {
                let actual = self.check_expr(expr);
                match &actual {
                    Some(LKitType::Ref(_)) | Some(LKitType::StrictRef(_)) => {
                        self.stmt_error(
                            stmt,
                            "Cannot return a handle — it would outlive its referent",
                        );
                    }
                    _ => {}
                }

                match (&self.current_return_type, actual) {
                    (Some(expected), Some(actual)) if actual != *expected => {
                        self.stmt_error(
                            stmt,
                            format!(
                                "Return type mismatch: expected {:?}, got {:?}",
                                expected, actual
                            ),
                        );
                    }
                    _ => {}
                }
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                match self.check_expr(condition) {
                    Some(t) if t != LKitType::Bool => {
                        self.stmt_error(stmt, format!("If condition must be Bool, got {:?}", t));
                    }
                    _ => {}
                }
                self.push_scope();
                self.check_stmt(then_branch);
                self.pop_scope();
                if let Some(else_stmt) = else_branch {
                    self.push_scope();
                    self.check_stmt(else_stmt);
                    self.pop_scope();
                }
            }

            Stmt::While { condition, body } => {
                match self.check_expr(condition) {
                    Some(t) if t != LKitType::Bool => {
                        self.stmt_error(stmt, format!("While condition must be Bool, got {:?}", t));
                    }
                    _ => {}
                }
                self.push_scope();
                self.check_stmt(body);
                self.pop_scope();
            }

            Stmt::Block(stmts) => {
                self.push_scope();
                for s in stmts {
                    self.check_stmt(s);
                }
                self.pop_scope();
            }

            Stmt::Extend { type_name, items } => {
                for item in items {
                    match item {
                        ExtendItem::Init { params, body } => {
                            self.push_scope();
                            // 'this' is implicit — define it as the struct type
                            self.define("this", LKitType::Struct(type_name.clone()));
                            for (p_name, p_type) in params {
                                if let Some(ty) = LKitType::from_str(p_type) {
                                    self.define(p_name, ty);
                                }
                            }
                            self.current_return_type = Some(LKitType::Struct(type_name.clone()));
                            for s in body {
                                self.check_stmt(s);
                            }
                            self.current_return_type = None;
                            self.pop_scope();
                        }
                        ExtendItem::Dinit { body } => {
                            self.push_scope();
                            self.define("this", LKitType::Struct(type_name.clone()));
                            self.current_return_type = Some(LKitType::Void);
                            for s in body {
                                self.check_stmt(s);
                            }
                            self.current_return_type = None;
                            self.pop_scope();
                        }
                        ExtendItem::Method {
                            name: _,
                            params,
                            return_type,
                            body,
                        } => {
                            self.push_scope();
                            for (p_name, p_type) in params {
                                if let Some(ty) = LKitType::from_str(p_type) {
                                    self.define(p_name, ty);
                                }
                            }
                            let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                            self.current_return_type = Some(ret);
                            for s in body {
                                self.check_stmt(s);
                            }
                            self.current_return_type = None;
                            self.pop_scope();
                        }
                    }
                }
            }

            Stmt::Expression(expr) => {
                self.check_expr(expr);
            }

            Stmt::Extern { .. } => {} // already registered in first pass
            Stmt::Struct { .. } => {} // already registered
            Stmt::Import { .. } => {} // imports done early
            Stmt::Tool { .. } => {}
            Stmt::ExtendWith {
                type_name,
                tool_name,
                items,
            } => {
                // validate type exists
                if !self.structs.contains_key(type_name) {
                    self.errors.push(TypeError::new(format!(
                        "Cannot extend unknown type '{}'",
                        type_name
                    )));
                    return;
                }

                // validate tool exists
                if !self.tools.contains_key(tool_name) {
                    self.stmt_error(stmt, format!("Unknown tool '{}'", tool_name));
                    return;
                }

                // type check each method body
                for item in items {
                    if let ExtendItem::Method {
                        name: _,
                        params,
                        return_type,
                        body,
                    } = item
                    {
                        self.push_scope();
                        for (p_name, p_type) in params {
                            if let Some(ty) = LKitType::from_str(p_type) {
                                self.define(p_name, ty);
                            }
                        }
                        let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                        self.current_return_type = Some(ret);
                        for s in body {
                            self.check_stmt(s);
                        }
                        self.current_return_type = None;
                        self.pop_scope();
                    }
                }
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Option<LKitType> {
        match expr {
            Expr::Literal(val) => Some(match val {
                Value::Int(_) => LKitType::Int,
                Value::Float(_) => LKitType::Float,
                Value::Bool(_) => LKitType::Bool,
                Value::Str(_) => LKitType::Str,
                Value::Null => LKitType::Void,
            }),

            Expr::Variable(name) => {
                match self.lookup(name) {
                    Some(ty) => match ty.clone() {
                        LKitType::Ref(inner) => Some(*inner),
                        LKitType::StrictRef(inner) => Some(*inner),
                        LKitType::HeapOwner(inner) => Some(*inner), // auto-deref
                        other => Some(other),
                    },
                    None => {
                        self.expr_error(expr, format!("Undefined variable '{}'", name));
                        None
                    }
                }
            }

            Expr::Assign { target, value } => {
                // 1. Determine the type of the assignment target
                let target_ty = match target.as_ref() {
                    Expr::Variable(name) => match self.lookup(name) {
                        Some(LKitType::StrictRef(inner)) => *inner.clone(),
                        Some(LKitType::HeapOwner(inner)) => *inner.clone(),
                        Some(LKitType::Ref(_)) => {
                            self.errors.push(TypeError::new(format!(
                                "Cannot assign through shared handle '{}'",
                                name
                            )));
                            return None;
                        }
                        Some(t) => t.clone(),
                        None => {
                            self.errors
                                .push(TypeError::new(format!("Undefined variable '{}'", name)));
                            return None;
                        }
                    },
                    Expr::FieldAccess { object, field } => {
                        let obj_ty = self.check_expr(object)?;
                        // unwrap handle, but reject shared handles
                        let base_ty = match obj_ty {
                            LKitType::StrictRef(inner) => *inner,
                            LKitType::HeapOwner(inner) => *inner,
                            LKitType::Ref(_) => {
                                self.errors.push(TypeError::new(
                                    "Cannot assign to field through shared handle",
                                ));
                                return None;
                            }
                            other => other,
                        };
                        match base_ty {
                            LKitType::Struct(name) => match self.structs.get(&name) {
                                Some(def) => match def.field_type(field) {
                                    Some(ty) => ty.clone(),
                                    None => {
                                        self.errors.push(TypeError::new(format!(
                                            "No field '{}' on struct '{}'",
                                            field, name
                                        )));
                                        return None;
                                    }
                                },
                                None => {
                                    self.errors
                                        .push(TypeError::new(format!("Unknown struct '{}'", name)));
                                    return None;
                                }
                            },
                            other => {
                                self.errors.push(TypeError::new(format!(
                                    "Cannot assign to field on non-struct type {:?}",
                                    other
                                )));
                                return None;
                            }
                        }
                    }
                    Expr::Index { object, index: _ } => match self.check_expr(object)? {
                        LKitType::Slice(inner, _) | LKitType::DynSlice(inner) => *inner,
                        other => {
                            self.errors.push(TypeError::new(format!(
                                "Cannot index-assign into non-slice type {:?}",
                                other
                            )));
                            return None;
                        }
                    },
                    _ => {
                        self.errors.push(TypeError::new(format!(
                            "Invalid assignment target: - {:?} - = {:?}",
                            target, value
                        )));
                        return None;
                    }
                };

                // 2. Type-check the value being assigned and compare to target
                match self.check_expr(value) {
                    Some(val_ty) if val_ty != target_ty => {
                        if val_ty == LKitType::Int && target_ty == LKitType::Byte {
                            Some(LKitType::Byte)
                        } else {
                            self.errors.push(TypeError::new(format!(
                                "Cannot assign {:?} to {:?}",
                                val_ty, target_ty
                            )));
                            None
                        }
                    }
                    other => other,
                }
            }

            Expr::Unary { op, operand } => match op {
                TokenType::Not => match self.check_expr(operand)? {
                    LKitType::Bool => Some(LKitType::Bool),
                    other => {
                        self.errors
                            .push(TypeError::new(format!("'!' on non-bool type {:?}", other)));
                        None
                    }
                },

                TokenType::Minus => match self.check_expr(operand)? {
                    LKitType::Int => Some(LKitType::Int),
                    LKitType::Float => Some(LKitType::Float),
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "Unary minus on non-numeric type {:?}",
                            other
                        )));
                        None
                    }
                },
                _ => {
                    self.errors.push(TypeError::new(format!(
                        "Bad unary application: {:?} {:?}",
                        op, operand
                    )));
                    None
                }
            },

            Expr::Binary { left, op, right } => {
                let l = self.check_expr(left)?;
                let r = self.check_expr(right)?;

                // resolve numeric type compatibility
                let result_ty = match (&l, &r) {
                    // exact match — always ok
                    (a, b) if a == b => l.clone(),
                    // numeric widening: byte op int -> int
                    (LKitType::Byte, LKitType::Int) | (LKitType::Int, LKitType::Byte) => {
                        LKitType::Int
                    }
                    // numeric widening: byte op float -> float
                    (LKitType::Byte, LKitType::Float) | (LKitType::Float, LKitType::Byte) => {
                        LKitType::Float
                    }
                    // int op float -> float
                    (LKitType::Int, LKitType::Float) | (LKitType::Float, LKitType::Int) => {
                        LKitType::Float
                    }
                    _ => {
                        self.errors.push(TypeError::new(format!(
                            "Binary op {:?} on mismatched types {:?} and {:?}",
                            op, l, r
                        )));
                        return None;
                    }
                };

                match op {
                    TokenType::EqualEqual
                    | TokenType::NotEqual
                    | TokenType::Less
                    | TokenType::LessEqual
                    | TokenType::Greater
                    | TokenType::GreaterEqual => Some(LKitType::Bool),
                    _ => Some(result_ty),
                }
            }

            Expr::Call { callee, args } => {
                let name = match callee.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => {
                        self.errors
                            .push(TypeError::new("Only direct calls supported"));
                        return None;
                    }
                };
                if self.structs.contains_key(&name) {
                    let extend_def = self.extends.get(&name).cloned();
                    match extend_def.and_then(|d| d.init_params) {
                        Some(params) => {
                            for (arg, expected) in args.iter().zip(params.iter()) {
                                match self.check_expr(arg) {
                                    Some(actual) if &actual != expected => {
                                        self.errors.push(TypeError::new(format!(
                                            "Init arg mismatch for '{}': expected {:?}, got {:?}",
                                            name, expected, actual
                                        )));
                                    }
                                    _ => {}
                                }
                            }
                            return Some(LKitType::Struct(name));
                        }
                        None => {
                            self.errors.push(TypeError::new(format!(
                                "Struct '{}' has no init defined",
                                name
                            )));
                            return None;
                        }
                    }
                }
                match self.functions.get(&name).cloned() {
                    Some(LKitType::Function { params, ret }) => {
                        if args.len() != params.len() {
                            // allow variadic mismatch for now
                        }
                        for (arg, expected) in args.iter().zip(params.iter()) {
                            match self.check_expr(arg) {
                                Some(actual) if &actual != expected => {
                                    self.errors.push(TypeError::new(format!(
                                        "Argument type mismatch in call to '{}': expected {:?}, got {:?}",
                                        name, expected, actual
                                    )));
                                }
                                _ => {}
                            }
                        }
                        Some(*ret)
                    }
                    _ => {
                        self.errors
                            .push(TypeError::new(format!("Undefined function '{}'", name)));
                        None
                    }
                }
            }

            Expr::Cast { target_type, expr } => {
                // still check the inner expression for undefined variables etc.
                self.check_expr(expr);
                LKitType::from_str(target_type)
            }
            Expr::FieldAccess { object, field } => {
                let obj_ty = self.check_expr(object)?;
                // unwrap handle
                let base_ty = match obj_ty {
                    LKitType::Ref(inner)
                    | LKitType::StrictRef(inner)
                    | LKitType::HeapOwner(inner) => *inner,
                    other => other,
                };
                match base_ty {
                    LKitType::Struct(name) => match self.structs.get(&name) {
                        Some(def) => match def.field_type(field) {
                            Some(ty) => Some(ty.clone()),
                            None => {
                                self.errors.push(TypeError::new(format!(
                                    "No field '{}' on struct '{}'",
                                    field, name
                                )));
                                None
                            }
                        },
                        None => {
                            self.errors
                                .push(TypeError::new(format!("Unknown struct '{}'", name)));
                            None
                        }
                    },
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot access field '{}' on non-struct type {:?}",
                            field, other
                        )));
                        None
                    }
                }
            }

            Expr::StructInit { name, fields } => {
                let def = match self.structs.get(name).cloned() {
                    Some(d) => d,
                    None => {
                        self.errors
                            .push(TypeError::new(format!("Unknown struct '{}'", name)));
                        return None;
                    }
                };
                // check each field value matches expected type
                for (i, (field_name, field_expr)) in fields.iter().enumerate() {
                    let expected = if field_name.is_empty() {
                        // positional
                        def.fields.get(i).map(|(_, t)| t.clone())
                    } else {
                        def.field_type(field_name).cloned()
                    };
                    match (expected, self.check_expr(field_expr)) {
                        (Some(exp), Some(actual)) if exp != actual => {
                            self.errors.push(TypeError::new(format!(
                                "Field '{}' of '{}': expected {:?}, got {:?}",
                                if field_name.is_empty() {
                                    format!("{}", i)
                                } else {
                                    field_name.to_string()
                                },
                                name,
                                exp,
                                actual
                            )));
                        }
                        _ => {}
                    }
                }
                Some(LKitType::Struct(name.clone()))
            }

            Expr::SliceLiteral(elements) => {
                if elements.is_empty() {
                    self.errors
                        .push(TypeError::new("Cannot infer type of empty slice literal"));
                    return None;
                }
                let first = self.check_expr(&elements[0])?;
                for el in &elements[1..] {
                    match self.check_expr(el) {
                        Some(t) if t != first => {
                            self.errors.push(TypeError::new(format!(
                                "Slice literal has mixed types: {:?} and {:?}",
                                first, t
                            )));
                            return None;
                        }
                        _ => {}
                    }
                }
                Some(LKitType::Slice(Box::new(first), elements.len() as u64))
            }

            Expr::Index { object, index } => {
                let idx_ty = self.check_expr(index)?;
                if idx_ty != LKitType::Int {
                    self.errors.push(TypeError::new("Slice index must be Int"));
                    return None;
                }

                if let Expr::Literal(Value::Int(n)) = index.as_ref() {
                    if let Some(LKitType::Slice(inner, size)) = self.check_expr(object) {
                        if *n < 0 || *n as u64 >= size {
                            self.errors.push(TypeError::new(format!(
                                "Index {} out of bounds for slice of size {}",
                                n, size
                            )));
                            return None;
                        }
                        return Some(*inner);
                    }
                }

                match self.check_expr(object)? {
                    LKitType::Slice(inner, _) => Some(*inner),
                    LKitType::DynSlice(inner) => Some(*inner),
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot index into non-slice type {:?}",
                            other
                        )));
                        None
                    }
                }
            }

            Expr::Len(expr) => match self.check_expr(expr)? {
                LKitType::Slice(_, _) | LKitType::DynSlice(_) => Some(LKitType::Int),
                other => {
                    self.errors.push(TypeError::new(format!(
                        "len() requires a slice, got {:?}",
                        other
                    )));
                    None
                }
            },
            Expr::Ref(inner) => match inner.as_ref() {
                Expr::Variable(name) => {
                    let ty = self
                        .lookup(name)
                        .ok_or_else(|| TypeError::new(format!("Undefined variable '{}'", name)))
                        .cloned();
                    match ty {
                        Ok(t) => {
                            if let Err(e) = self.borrow_shared(name) {
                                self.errors.push(e);
                                None
                            } else {
                                Some(LKitType::Ref(Box::new(t)))
                            }
                        }
                        Err(e) => {
                            self.errors.push(e);
                            None
                        }
                    }
                }
                _ => {
                    self.expr_error(expr, "Can only take handle of a variable");
                    None
                }
            },

            Expr::StrictRef(inner) => match inner.as_ref() {
                Expr::Variable(name) => {
                    let ty = self
                        .lookup(name)
                        .ok_or_else(|| TypeError::new(format!("Undefined variable '{}'", name)))
                        .cloned();
                    match ty {
                        Ok(t) => {
                            if let Err(e) = self.borrow_exclusive(name) {
                                self.errors.push(e);
                                None
                            } else {
                                Some(LKitType::StrictRef(Box::new(t)))
                            }
                        }
                        Err(e) => {
                            self.errors.push(e);
                            None
                        }
                    }
                }
                _ => {
                    self.expr_error(expr, "Can only take strict handle of a variable");
                    None
                }
            },

            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                if let Expr::Variable(name) = object.as_ref() {
                    // clone to release the borrow on self
                    let exports = self.module_exports.get(name.as_str()).cloned();
                    if let Some(exports) = exports {
                        let sig = match exports.get(method.as_str()) {
                            Some(LKitType::Function { params, ret }) => {
                                Some((params.clone(), ret.clone()))
                            }
                            _ => None,
                        };
                        match sig {
                            Some((params, ret)) => {
                                for (arg, expected) in args.iter().zip(params.iter()) {
                                    match self.check_expr(arg) {
                                        Some(actual) if &actual != expected => {
                                            self.errors.push(TypeError::new(format!(
                                                "Arg mismatch in {}.{}: expected {:?} got {:?}",
                                                name, method, expected, actual
                                            )));
                                        }
                                        _ => {}
                                    }
                                }
                                return Some(*ret);
                            }
                            None => {
                                self.errors.push(TypeError::new(format!(
                                    "No function '{}' in module '{}'",
                                    method, name
                                )));
                                return None;
                            }
                        }
                    }
                }

                let obj_ty = self.check_expr(object)?;
                // unwrap handle
                let base_ty = match obj_ty {
                    LKitType::Ref(inner) | LKitType::StrictRef(inner) => *inner,
                    other => other,
                };
                let type_name = match &base_ty {
                    LKitType::Struct(n) => n.clone(),
                    other => {
                        self.errors.push(TypeError::new(format!(
                            "Cannot call method on non-struct type {:?}",
                            other
                        )));
                        return None;
                    }
                };
                let extend_def = match self.extends.get(&type_name) {
                    Some(d) => d.clone(),
                    None => {
                        self.errors.push(TypeError::new(format!(
                            "No extend block for type '{}'",
                            type_name
                        )));
                        return None;
                    }
                };
                let sig = match extend_def.methods.get(method) {
                    Some(s) => s.clone(),
                    None => {
                        self.errors.push(TypeError::new(format!(
                            "No method '{}' on type '{}'",
                            method, type_name
                        )));
                        return None;
                    }
                };
                // check args — skip first param (this)
                let expected_params = &sig.params[1..];
                for (arg, expected) in args.iter().zip(expected_params.iter()) {
                    match self.check_expr(arg) {
                        Some(actual) if &actual != expected => {
                            self.errors.push(TypeError::new(format!(
                            "Argument type mismatch in call to '{}::{}': expected {:?}, got {:?}",
                            type_name, method, expected, actual
                        )));
                        }
                        _ => {}
                    }
                }
                Some(sig.ret.clone())
            }
        }
    }

    pub fn transform(&mut self, stmts: Vec<Stmt>) -> Vec<Stmt> {
        stmts.into_iter().map(|s| self.transform_stmt(s)).collect()
    }

    fn transform_stmt(&mut self, stmt: Stmt) -> Stmt {
        match stmt {
            Stmt::LetDecl { name, initializer } => {
                // Look up the inferred type from the scope
                let ty = self
                    .lookup(&name)
                    .map(|t| t.to_str().to_string())
                    .unwrap_or_else(|| "Int".to_string());
                Stmt::VarDecl {
                    name,
                    value_type: ty,
                    initializer,
                }
            }

            Stmt::Function {
                name,
                params,
                return_type,
                body,
            } => {
                self.push_scope();
                for (param_name, param_type) in &params {
                    if let Some(ty) = LKitType::from_str(param_type) {
                        self.define(param_name, ty);
                    }
                }
                let body = self.transform(body);
                self.pop_scope();
                Stmt::Function {
                    name,
                    params,
                    return_type,
                    body,
                }
            }

            Stmt::Block(stmts) => {
                self.push_scope();
                let stmts = self.transform(stmts);
                self.pop_scope();
                Stmt::Block(stmts)
            }

            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.push_scope();
                let then_branch = Box::new(self.transform_stmt(*then_branch));
                self.pop_scope();
                let else_branch = else_branch.map(|e| {
                    self.push_scope();
                    let e = Box::new(self.transform_stmt(*e));
                    self.pop_scope();
                    e
                });
                Stmt::If {
                    condition,
                    then_branch,
                    else_branch,
                }
            }

            Stmt::While { condition, body } => {
                self.push_scope();
                let body = Box::new(self.transform_stmt(*body));
                self.pop_scope();
                Stmt::While { condition, body }
            }

            // Everything else passes through unchanged
            other => other,
        }
    }

    pub fn register_module(&mut self, name: &str, stmts: &[Stmt]) {
        // run struct/extend/function registration passes
        self.register_pass(stmts);

        // collect exports: functions, structs, extends
        let mut exports = HashMap::new();
        for stmt in stmts {
            match stmt {
                Stmt::Function {
                    name: fn_name,
                    params,
                    return_type,
                    ..
                } => {
                    let param_types = params
                        .iter()
                        .filter_map(|(_, ty)| LKitType::from_str(ty))
                        .collect();
                    let ret = LKitType::from_str(return_type).unwrap_or(LKitType::Void);
                    exports.insert(
                        fn_name.clone(),
                        LKitType::Function {
                            params: param_types,
                            ret: Box::new(ret),
                        },
                    );
                }
                Stmt::Struct { name: sname, .. } => {
                    exports.insert(sname.clone(), LKitType::Struct(sname.clone()));
                }
                _ => {}
            }
        }
        self.module_exports.insert(name.to_string(), exports);
    }

    // helper methods:
    fn borrow_shared(&mut self, name: &str) -> Result<(), TypeError> {
        let state = self
            .borrow_state
            .entry(name.to_string())
            .or_insert((0, false));
        if state.1 {
            Err(TypeError::new(format!(
                "Cannot create shared handle to '{}' — exclusive handle exists",
                name
            )))
        } else {
            state.0 += 1;
            Ok(())
        }
    }

    fn borrow_exclusive(&mut self, name: &str) -> Result<(), TypeError> {
        let state = self
            .borrow_state
            .entry(name.to_string())
            .or_insert((0, false));
        if state.1 {
            Err(TypeError::new(format!(
                "Cannot create exclusive handle to '{}' — exclusive handle already exists",
                name
            )))
        } else if state.0 > 0 {
            Err(TypeError::new(format!(
                "Cannot create exclusive handle to '{}' — shared handles exist",
                name
            )))
        } else {
            state.1 = true;
            Ok(())
        }
    }

    pub fn implements(&self, type_name: &str, tool_name: &str) -> bool {
        self.implementations
            .get(type_name)
            .map(|tools| tools.contains(tool_name))
            .unwrap_or(false)
    }
}
