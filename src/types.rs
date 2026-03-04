use std::vec::Vec;
use std::boxed::Box;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum LKitType {
    Int,
    Float,
    Bool,
    Str,
    Ptr,
    Void,
    Byte,
    Slice(Box<LKitType>, u64),
    DynSlice(Box<LKitType>),
    Ref(Box<LKitType>),        // T&
    StrictRef(Box<LKitType>),  // T strict&
    Struct(String),
    HeapOwner(Box<LKitType>),  // T*
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
            "Byte" => Some(LKitType::Byte),
            "Ptr" => Some(LKitType::Ptr),
            other if other.ends_with('&') => {
                let inner = other.trim_end_matches('&').trim();
                if inner.ends_with("strict") {
                    let base = inner.trim_end_matches("strict").trim();
                    LKitType::from_str(base).map(|t| LKitType::StrictRef(Box::new(t)))
                } else {
                    LKitType::from_str(inner).map(|t| LKitType::Ref(Box::new(t)))
                }
            }
            other if other.ends_with('*') => {
                let inner = other.trim_end_matches('*').trim();
                LKitType::from_str(inner).map(|t| LKitType::HeapOwner(Box::new(t)))
            }

            other if other.starts_with('[') => {
                // [T]
                let inner = &other[1..other.len()-1];
                LKitType::from_str(inner).map(|t| LKitType::DynSlice(Box::new(t)))
            }
            other => {
                // T[N] or struct name
                if let Some(bracket) = other.find('[') {
                    let base = &other[..bracket];
                    let size_str = &other[bracket+1..other.len()-1];
                    if let (Some(base_ty), Ok(n)) 
                    = (LKitType::from_str(base), size_str.parse::<u64>()) {
                        return Some(LKitType::Slice(Box::new(base_ty), n));
                    }
                }
                Some(LKitType::Struct(other.to_string()))
            }
        }
    }


   pub fn to_str(&self) -> String {
        match self {
            LKitType::Int     => "Int".to_string(),
            LKitType::Float   => "Float".to_string(),
            LKitType::Bool    => "Bool".to_string(),
            LKitType::Str     => "Str".to_string(),
            LKitType::Void    => "Void".to_string(),
            LKitType::Ptr     => "Ptr".to_string(),
            LKitType::Byte => "Byte".to_string(),
            LKitType::Slice(inner, n) => format!("{}[{}]", inner.to_str(), n),
            LKitType::DynSlice(inner) => format!("[{}]", inner.to_str()),
            LKitType::Struct(name) => name.clone(),
            LKitType::Function { .. } => "Function".to_string(),
            LKitType::Ref(inner)       => format!("{}&", inner.to_str()),
            LKitType::StrictRef(inner) => format!("{} strict&", inner.to_str()),
            LKitType::HeapOwner(inner) => format!("{}*", inner.to_str()),
        }
    }
    #[allow(dead_code)]
    pub fn is_copy(&self, structs: &HashMap<String, StructDef>) -> bool {
        match self {
            LKitType::Int | LKitType::Float | LKitType::Bool => true,
            LKitType::Str => false,
            LKitType::Void => true,
            LKitType::Ptr => false,
            LKitType::Byte => true,
            LKitType::Slice(_, _) => false,
            LKitType::DynSlice(_) => false,
            LKitType::Function { .. } => false,
            LKitType::Struct(name) => {
                match structs.get(name) {
                    Some(def) => def.is_copy(structs),
                    None => false,
                }
            }
            LKitType::Ref(_)       => true,  // shared read handle is copyable
            LKitType::StrictRef(_) => false, // exclusive handle is not
            LKitType::HeapOwner(_) => false,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
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
        LKitType::Ptr   => 8, 
        LKitType::Void  => 0,
        LKitType::Byte => 1,
        LKitType::Slice(_,size) => *size as usize,
        LKitType::DynSlice(_) => 24, // ptr, len, 
        LKitType::Struct(name) => structs.get(name)
            .map(|d| d.size_bytes(structs))
            .unwrap_or(0),
        LKitType::Function { .. } => 8, // pointer
        LKitType::Ref(_) | LKitType::StrictRef(_) => 8,
        LKitType::HeapOwner(_) => 8, // always pointer sized
    }
}
