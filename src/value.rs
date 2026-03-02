use std::sync::Arc;
use crate::ast::Stmt;

#[derive(Clone, Debug)]
pub struct FunctionData {
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String), // This owns its memory! No strndup needed.
    Bool(bool),
    Function(Arc<FunctionData>), 
    Null,
}

impl Value {
    // Helper to print values, similar to your C switch statement
    pub fn to_string(&self) -> String {
        match self {
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Str(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Null => "null".to_string(),
            Value::Function(_) => "Function".to_string(),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Function(a), Value::Function(b)) 
                => Arc::ptr_eq(a, b), // Pointer equality!
            _ => false,
        }
    }
}
