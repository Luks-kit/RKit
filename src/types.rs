use std::vec::Vec;
use std::boxed::Box;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LKitType {
    Int,
    Float,
    Bool,
    Str,
    Void,
    Slice(Box<LKitType>),
    Struct(String),
    Function {
        params: Vec<LKitType>,
        ret: Box<LKitType>,
    },
}

impl LKitType {
    pub fn from_str(s: &str) -> Option<LKitType> {
        match s {
            "Int"   => Some(LKitType::Int),
            "Float" => Some(LKitType::Float),
            "Bool"  => Some(LKitType::Bool),
            "Str"   => Some(LKitType::Str),
            "Void"  => Some(LKitType::Void),
            other   => Some(LKitType::Struct(other.to_string()))
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            LKitType::Int     => "Int",
            LKitType::Float   => "Float",
            LKitType::Bool    => "Bool",
            LKitType::Str     => "Str",
            LKitType::Void    => "Void",
            LKitType::Slice(_) => "Slice",
            LKitType::Function { .. } => "Function",
            LKitType::Struct(name) => name,
        }
    }
    pub fn is_copy(&self, structs: &HashMap<String, StructDef>) -> bool {
        match self {
            LKitType::Int | LKitType::Float | LKitType::Bool => true,
            LKitType::Str => false,
            LKitType::Void => true,
            LKitType::Slice(_) => false,
            LKitType::Function { .. } => false,
            LKitType::Struct(name) => {
                match structs.get(name) {
                    Some(def) => def.is_copy(structs),
                    None => false,
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, LKitType)>,
}

impl StructDef {
    pub fn size_bytes(&self, structs: &HashMap<String, StructDef>) -> usize {
        self.fields.iter().map(|(_, ty)| ty_size(ty, structs)).sum()
    }

    pub fn is_copy(&self, structs: &HashMap<String, StructDef>) -> bool {
        self.size_bytes(structs) <= 8
            && self.fields.iter().all(|(_, ty)| ty.is_copy(structs))
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|(n, _)| n == name)
    }

    pub fn field_type(&self, name: &str) -> Option<&LKitType> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

pub fn ty_size(ty: &LKitType, structs: &HashMap<String, StructDef>) -> usize {
    match ty {
        LKitType::Int   => 8,
        LKitType::Float => 8,
        LKitType::Bool  => 1,
        LKitType::Str   => 8, // pointer
        LKitType::Void  => 0,
        LKitType::Slice(_) => 16, // ptr + len
        LKitType::Struct(name) => structs.get(name)
            .map(|d| d.size_bytes(structs))
            .unwrap_or(0),
        LKitType::Function { .. } => 8, // pointer
    }
}
