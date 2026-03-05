use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::types::BasicType;
use inkwell::values::*;
use inkwell::types::BasicTypeEnum;
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::{IntPredicate, FloatPredicate};
use inkwell::AddressSpace;
use std::collections::{HashSet, HashMap};
use crate::value::Value;
use crate::lexer::TokenType;
use crate::ast::{Expr, Stmt, ExtendItem};
use crate::types::StructDef;
use crate::types::LKitType;


pub struct VarSlot<'ctx> {
    pub ptr: PointerValue<'ctx>,
    pub ty: BasicTypeEnum<'ctx>,
    pub is_ref: bool,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct CompiledExtend {
    pub init_fn: Option<String>,   // mangled name: "TypeName__init"
    pub dinit_fn: Option<String>,  // mangled name: "TypeName__dinit"
    pub methods: HashMap<String, String>, // method name -> mangled name
}

pub struct Compiler<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    pub struct_defs: HashMap<String, StructDef>,
    pub modules: HashSet<String>,
    pub variables: Vec<HashMap<String, VarSlot<'ctx>>>,
    pub extends: HashMap<String, CompiledExtend>,

}

impl<'ctx> Compiler<'ctx> {
    
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        Compiler {
            context,
            module: context.create_module(module_name),
            builder: context.create_builder(),
            variables: vec![HashMap::new()],  // start with one scope
            modules: HashSet::new(),
            struct_defs: HashMap::new(),
            extends: HashMap::new(),
        }
    }

    pub fn compile_expression(&self, expr: Expr) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            Expr::Literal(val) => match val {
                Value::Int(i)   => Ok(self.context.i64_type().const_int(i as u64, true).as_basic_value_enum()),
                Value::Float(f) => Ok(self.context.f64_type().const_float(f).as_basic_value_enum()),
                Value::Bool(b)  => Ok(self.context.bool_type().const_int(b as u64, false).as_basic_value_enum()),
                Value::Str(s)   => {
                    let global = self.builder.build_global_string_ptr(&s, "str")
                        .map_err(|e| e.to_string())?;
                    Ok(global.as_pointer_value().as_basic_value_enum())
                },
                Value::Null        => Ok(self.context.i64_type().const_int(0, false).as_basic_value_enum()),
            },

            Expr::Variable(name) => {
                let slot = self.lookup_var(&name)
                    .ok_or_else(|| format!("Undefined variable: {}", name))?;
                let val = self.builder.build_load(slot.ty, slot.ptr, &name)
                    .map_err(|e| e.to_string())?;
                if slot.is_ref {
                    // strip handle wrapper to get pointee type
                    let pointee_type_name = slot.type_name
                        .trim_end_matches('&')
                        .trim_end_matches("strict")
                        .trim()
                        .to_string();
                    let pointee_ty = self.type_str_to_llvm(&pointee_type_name);
                    self.builder.build_load(pointee_ty, val.into_pointer_value(), &name)
                        .map_err(|e| e.to_string())
                } else if slot.type_name.ends_with('*') {
                    // heap owner — auto-deref
                    let pointee_type_name = slot.type_name.trim_end_matches('*').trim().to_string();
                    let pointee_ty = self.type_str_to_llvm(&pointee_type_name);
                    self.builder.build_load(pointee_ty, val.into_pointer_value(), &name)
                        .map_err(|e| e.to_string())
                } else {
                    Ok(val)
                }
            }

           Expr::Assign { target, value } => {
                let val = self.compile_expression(*value)?;
                let (ptr, pointee_ty) = self.compile_lvalue(&target)?;
                // truncate if value type is wider than pointee type
                let val = match (val, pointee_ty) {
                    (BasicValueEnum::IntValue(i), BasicTypeEnum::IntType(t))
                        if i.get_type().get_bit_width() > t.get_bit_width() => {
                        self.builder.build_int_truncate(i, t, "trunc")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum()
                    }
                    (v, _) => v,
                };
                self.builder.build_store(ptr, val)
                    .map_err(|e| e.to_string())?;
                Ok(val)
            } 
        
            Expr::Cast { target_type, expr } => {
                let val = self.compile_expression(*expr)?;
                let target_ty = self.type_str_to_llvm(&target_type);
                
                match (val, target_ty) {
                    // ptr -> ptr (T* to U*, ptr to T*, etc.) — just bitcast
                    (BasicValueEnum::PointerValue(p), BasicTypeEnum::PointerType(_)) => {
                        Ok(p.as_basic_value_enum()) // opaque pointers, no-op in LLVM 15+
                    }
                    // int -> ptr
                    (BasicValueEnum::IntValue(i), BasicTypeEnum::PointerType(t)) => {
                        Ok(self.builder.build_int_to_ptr(i, t, "cast")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum())
                    }
                    // ptr -> int
                    (BasicValueEnum::PointerValue(p), BasicTypeEnum::IntType(t)) => {
                        Ok(self.builder.build_ptr_to_int(p, t, "cast")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum())
                    }
                    // int -> int (truncate or extend)
                    (BasicValueEnum::IntValue(i), BasicTypeEnum::IntType(t)) => {
                        Ok(self.builder.build_int_cast(i, t, "cast")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum())
                    }
                    // float -> int
                    (BasicValueEnum::FloatValue(f), BasicTypeEnum::IntType(t)) => {
                        Ok(self.builder.build_float_to_signed_int(f, t, "cast")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum())
                    }
                    // int -> float
                    (BasicValueEnum::IntValue(i), BasicTypeEnum::FloatType(t)) => {
                        Ok(self.builder.build_signed_int_to_float(i, t, "cast")
                            .map_err(|e| e.to_string())?
                            .as_basic_value_enum())
                    }
                    (v, t) => Err(format!("Cannot cast {:?} to {:?}", v.get_type(), t)),
                }
            }

           Expr::Unary { op, operand } => {
                let val = self.compile_expression(*operand)?;
                match op {
                    TokenType::Minus => match val {
                        BasicValueEnum::IntValue(v) =>
                            Ok(self.builder.build_int_neg(v, "negtmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        BasicValueEnum::FloatValue(v) =>
                            Ok(self.builder.build_float_neg(v, "fnegtmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        _ => Err("Unary minus on non-numeric value".into()),
                    },
                    TokenType::Not => match val {
                        BasicValueEnum::IntValue(v) =>
                            Ok(self.builder.build_not(v, "nottmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                        _ => Err("'!' on non-bool value".into()),
                    },
                    _ => Err(format!("Unknown unary operator {:?}", op)),
                }
            }

           Expr::Binary { left, op, right } => {
                let lhs = self.compile_expression(*left)?;
                let rhs = self.compile_expression(*right)?;
                match (lhs, rhs) {
                    (BasicValueEnum::IntValue(l), BasicValueEnum::IntValue(r)) => {
                        let result = match op {
                            // Arithmetic: Wrap the result in BasicValueEnum to match the comparison arms
                            TokenType::Plus => self.builder.build_int_add(l, r, "addtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Minus => self.builder.build_int_sub(l, r, "subtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Star => self.builder.build_int_mul(l, r, "multmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Slash => self.builder.build_int_signed_div(l, r, "divtmp")
                                .map(|v| v.as_basic_value_enum()),

                            // Comparisons: These already return BasicValueEnum because of your .map()
                            TokenType::EqualEqual => self.builder.build_int_compare(IntPredicate::EQ, l, r, "eqtmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::NotEqual => self.builder.build_int_compare(IntPredicate::NE, l, r, "netmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Less => self.builder.build_int_compare(IntPredicate::SLT, l, r, "lttmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::LessEqual => self.builder.build_int_compare(IntPredicate::SLE, l, r, "letmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::Greater => self.builder.build_int_compare(IntPredicate::SGT, l, r, "gttmp")
                                .map(|v| v.as_basic_value_enum()),
                            TokenType::GreaterEqual => self.builder.build_int_compare(IntPredicate::SGE, l, r, "getmp")
                                .map(|v| v.as_basic_value_enum()),

                            _ => return Err(format!("Unknown operator {:?}", op)),
                    };

                // Now 'result' is consistently Result<BasicValueEnum, BuilderError>
                result.map_err(|e| e.to_string()) 
                    },
                    (BasicValueEnum::FloatValue(l), BasicValueEnum::FloatValue(r)) => {
                        let result = match op {
                            TokenType::Plus  => self.builder.build_float_add(l, r, "faddtmp"),
                            TokenType::Minus => self.builder.build_float_sub(l, r, "fsubtmp"),
                            TokenType::Star  => self.builder.build_float_mul(l, r, "fmultmp"),
                            TokenType::Slash => self.builder.build_float_div(l, r, "fdivtmp"),
                            TokenType::EqualEqual => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OEQ,l,r,"feqtmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::NotEqual  => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::ONE,l,r,"fnetmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                            TokenType::Less       => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OLT,l,r,"flttmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::LessEqual  => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OLE,l,r,"fletmp")
                                .map_err(|e| e.to_string())?
                                .as_basic_value_enum()),
                            TokenType::Greater    => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OGT,l,r,"fgttmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            TokenType::GreaterEqual => 
                                return Ok(self.builder.build_float_compare(FloatPredicate::OGE,l,r,"fgetmp")
                                    .map_err(|e| e.to_string())?
                                    .as_basic_value_enum()),
                            _ => return Err(format!("Unknown operator {:?}", op)),
                        };
                        result.map(|v| v.as_basic_value_enum()).map_err(|e| e.to_string())
                    },
                    _ => Err("Type mismatch in binary expression".into()),
                }
            },

            Expr::Call { callee, args } => {
                let callee_name = match *callee {
                    Expr::Variable(name) => name,
                    _ => return Err("Only direct function calls supported".into()),
                };
                
                // check if it's a struct init call
                if let Some(extend) = self.extends.get(&callee_name).cloned() {
                    if let Some(init_name) = extend.init_fn {
                        let function = self.module.get_function(&init_name)
                            .ok_or_else(|| format!("Init for '{}' not compiled", callee_name))?;
                        let compiled_args: Vec<BasicMetadataValueEnum> = args.into_iter()
                            .map(|a| self.compile_expression(a).map(|v| v.into()))
                            .collect::<Result<_, _>>()?;
                        let call = self.builder.build_call(function, &compiled_args, "initcall")
                            .map_err(|e| e.to_string())?;
                        return match call.try_as_basic_value() {
                            ValueKind::Basic(val) => Ok(val),
                            ValueKind::Instruction(_) => Err("Init returned void".into()),
                        };
                    }
                }

                let function = self.module.get_function(&callee_name)
                    .ok_or_else(|| format!("Undefined function: {}", callee_name))?;
                let compiled_args: Vec<BasicMetadataValueEnum> = args.into_iter()
                    .map(|a| self.compile_expression(a).map(|v| v.into()))
                    .collect::<Result<_, _>>()?;
                let call = self.builder.build_call(function, &compiled_args, "calltmp")
                    .map_err(|e| e.to_string())?;
                match call.try_as_basic_value() {
                    ValueKind::Basic(val) => Ok(val),
                    ValueKind::Instruction(_) => Ok(self.context.i64_type().const_int(0, false).as_basic_value_enum()),
                }   
            },

            Expr::StructInit { name, fields } => {
                let struct_ty = self.get_struct_type(&name)?;
                let alloca = self.builder.build_alloca(struct_ty, &name)
                    .map_err(|e| e.to_string())?;
                let base_name = name
                    .trim_end_matches('*')
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                   .trim();
                let _def = self.struct_defs.get(base_name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", name))?;
                for (i, (_, val_expr)) in fields.iter().enumerate() {
                    let val = self.compile_expression(val_expr.clone())?;
                    let ptr = self.builder.build_struct_gep(struct_ty, alloca, i as u32, "field")
                        .map_err(|e| e.to_string())?;
                    self.builder.build_store(ptr, val)
                        .map_err(|e| e.to_string())?;
                }
                Ok(self.builder.build_load(struct_ty, alloca, &name)
                    .map_err(|e| e.to_string())?)
            }
            
            Expr::FieldAccess { object, field } => {
                let vname = match *object {
                    Expr::Variable(ref vname) => vname.clone(),
                    _ => return Err("Complex field access not yet supported".into()),
                };
                let slot = self.lookup_var(&vname)
                    .ok_or_else(|| format!("Undefined variable '{}'", vname))?;

                // resolve struct name from type_name, stripping any handle wrapper
                let struct_name = slot.type_name
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim()
                    .to_string();

                // if it's a handle or an owner, load the pointer first
                let struct_ptr = if slot.is_ref || slot.type_name.ends_with('*') {
                    self.builder.build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        slot.ptr,
                        &vname
                    ).map_err(|e| e.to_string())?.into_pointer_value()
                } else {
                    slot.ptr
                };
                let base_name = struct_name
                    .trim_end_matches('*')
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim();
                let def = self.struct_defs.get(base_name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", base_name))?;
                let idx = def.field_index(&field)
                    .ok_or_else(|| format!("No field '{}' on '{}'", field, struct_name))? as u32;
                let struct_ty = self.get_struct_type(&struct_name)?;
                let field_ty = self.type_str_to_llvm(&def.fields[idx as usize].1.to_str());

                let field_ptr = self.builder.build_struct_gep(struct_ty, struct_ptr, idx, &field)
                    .map_err(|e| e.to_string())?;
                self.builder.build_load(field_ty, field_ptr, &field)
                    .map_err(|e| e.to_string())
            },

            Expr::SliceLiteral(elements) => {
                if elements.is_empty() {
                    return Err("Cannot compile empty slice literal".into());
                }
                let compiled: Vec<BasicValueEnum> = elements.into_iter()
                    .map(|e| self.compile_expression(e))
                    .collect::<Result<_, _>>()?;
                let elem_ty = compiled[0].get_type();
                let n = compiled.len() as u32;
                let array_ty = match elem_ty {
                    BasicTypeEnum::IntType(t)   => t.array_type(n),
                    BasicTypeEnum::FloatType(t) => t.array_type(n),
                    BasicTypeEnum::StructType(t) => t.array_type(n),
                    BasicTypeEnum::PointerType(t) => t.array_type(n),
                    BasicTypeEnum::ArrayType(t) => t.array_type(n),
                    BasicTypeEnum::VectorType(t) => t.array_type(n),
                    _ => return Err("Unsupported element type in slice literal".into()),
                }; 
                let alloca = self.builder.build_alloca(array_ty, "slice")
                    .map_err(|e| e.to_string())?;
                for (i, val) in compiled.iter().enumerate() {
                    let ptr = unsafe {
                        self.builder.build_gep(array_ty, alloca, &[
                            self.context.i64_type().const_int(0, false),
                            self.context.i64_type().const_int(i as u64, false),
                        ], "elem_ptr").map_err(|e| e.to_string())?
                    };
                    self.builder.build_store(ptr, *val)
                        .map_err(|e| e.to_string())?;
                }
                self.builder.build_load(array_ty, alloca, "slice_val")
                    .map_err(|e| e.to_string())
            }

            Expr::Index { object, index } => {
                let name = match *object {
                    Expr::Variable(ref n) => n.clone(),
                    _ => return Err("Complex index expressions not yet supported".into()),
                };
                let slot = self.lookup_var(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                let is_const_index = matches!(*index, Expr::Literal(Value::Int(_)));
                let idx_val = self.compile_expression(*index)?.into_int_value(); 

                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        if !is_const_index {
                            let len = arr_ty.len();
                            // bounds check
                            let len_val = self.context.i64_type().const_int(len as u64, false);
                            let in_bounds = self.builder.build_int_compare(
                                IntPredicate::ULT, idx_val, len_val, "bounds_check"
                            ).map_err(|e| e.to_string())?;
                            let func = self.builder.get_insert_block()
                                .unwrap().get_parent().unwrap();
                            let ok_bb    = self.context.append_basic_block(func, "inbounds");
                            let fail_bb  = self.context.append_basic_block(func, "outofbounds");
                            self.builder.build_conditional_branch(in_bounds, ok_bb, fail_bb)
                                .map_err(|e| e.to_string())?;

                            // out of bounds — call abort
                            self.builder.position_at_end(fail_bb);
                            let abort = self.module.get_function("abort").unwrap_or_else(|| {
                                let ty = self.context.void_type().fn_type(&[], false);
                                self.module.add_function("abort", ty, Some(inkwell::module::Linkage::External))
                            });
                            self.builder.build_call(abort, &[], "")
                                .map_err(|e| e.to_string())?;
                            self.builder.build_unreachable()
                                .map_err(|e| e.to_string())?;

                            self.builder.position_at_end(ok_bb);
                        }
                        let elem_ty = arr_ty.get_element_type();
                        let ptr = unsafe {
                            self.builder.build_gep(slot.ty, slot.ptr, &[
                                self.context.i64_type().const_int(0, false),
                                idx_val,
                            ], "elem_ptr").map_err(|e| e.to_string())?
                        };
                        self.builder.build_load(elem_ty, ptr, "elem")
                            .map_err(|e| e.to_string())
                    }
                    _ => Err(format!("Cannot index into non-array variable '{}'", name)),
                }
            }

            Expr::Len(expr) => {
                let name = match *expr {
                    Expr::Variable(n) => n,
                    _ => return Err("len() argument must be a variable".into()),
                };
                let slot = self.lookup_var(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        Ok(self.context.i64_type()
                            .const_int(arr_ty.len() as u64, false)
                            .as_basic_value_enum())
                    }
                    _ => Err(format!("len() requires a slice, '{}' is not an array", name)),
                }
            }
            Expr::Ref(inner) | Expr::StrictRef(inner) => {
                match *inner {
                    Expr::Variable(name) => {
                        let slot = self.lookup_var(&name)
                            .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                        Ok(slot.ptr.as_basic_value_enum())
                    }
                    _ => Err("Can only take handle of a variable".into()),
                }
            }
           Expr::MethodCall { object, method, args } => {
                let var_name = match *object {
                    Expr::Variable(ref n) => n.clone(),
                    _ => return Err("Complex method calls not yet supported".into()),
                };

                // check if it's a module call
                if self.modules.contains(&var_name) {
                    let mangled = format!("{}__{}", var_name, method);
                    let function = self.module.get_function(&mangled)
                        .ok_or_else(|| format!("No function '{}.{}' found", var_name, method))?;
                    let compiled_args: Vec<BasicMetadataValueEnum> = args.into_iter()
                        .map(|a| self.compile_expression(a).map(|v| v.into()))
                        .collect::<Result<_, _>>()?;
                    let call = self.builder.build_call(function, &compiled_args, "modcall")
                        .map_err(|e| e.to_string())?;
                    return match call.try_as_basic_value() {
                        ValueKind::Basic(val) => Ok(val),
                        ValueKind::Instruction(_) => Ok(self.context.i64_type()
                            .const_int(0, false).as_basic_value_enum()),
                    };
                }

                // otherwise it's a method call on a struct
                let slot = self.lookup_var(&var_name)
                    .ok_or_else(|| format!("Undefined variable '{}'", var_name))?;
                let struct_name = slot.type_name
                    .trim_end_matches('*')
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim()
                    .to_string();
                let mangled = self.extends.get(&struct_name)
                    .and_then(|e| e.methods.get(&method))
                    .cloned()
                    .ok_or_else(|| format!("No method '{}' on '{}'", method, struct_name))?;
                let function = self.module.get_function(&mangled)
                    .ok_or_else(|| format!("Method '{}' not compiled", mangled))?;
                let mut compiled_args: Vec<BasicMetadataValueEnum> = vec![slot.ptr.into()];
                for arg in args {
                    compiled_args.push(self.compile_expression(arg)?.into());
                }
                let call = self.builder.build_call(function, &compiled_args, "methodcall")
                    .map_err(|e| e.to_string())?;
                match call.try_as_basic_value() {
                    ValueKind::Basic(val) => Ok(val),
                    ValueKind::Instruction(_) => Ok(self.context.i64_type()
                        .const_int(0, false).as_basic_value_enum()),
                }
            } 
           
        }
    }
    
    pub fn compile_statement(&mut self, stmt: Stmt) -> Result<(), String> {
        match stmt {
            Stmt::VarDecl { name, value_type, initializer } => {
                let initial_val = self.compile_expression(initializer)?;
                let is_ref = value_type.ends_with('&');
                let ty = if is_ref {
                    self.context.ptr_type(AddressSpace::default()).into()
                } else {
                    initial_val.get_type()
                };
                let alloca = self.builder.build_alloca(ty, &name)
                    .map_err(|e| e.to_string())?;
                self.builder.build_store(alloca, initial_val)
                    .map_err(|e| e.to_string())?;
                self.define_var(name, VarSlot { ptr: alloca, ty, is_ref, type_name: value_type.clone() });
                Ok(())
            }

            Stmt::Expression(expr) => {
                self.compile_expression(expr)?;
                Ok(())
            }
            
           Stmt::Return(Expr::Literal(Value::Null)) => {
                let scopes: Vec<Vec<(String, PointerValue<'ctx>)>> = self.variables.iter()
                    .map(|scope| scope.iter()
                        .map(|(_, slot)| (slot.type_name.clone(), slot.ptr))
                        .collect())
                    .collect();
                for scope_vars in scopes.iter().rev() {
                    self.emit_dinits_for_vars(scope_vars)?;
                }
                self.builder.build_return(None)
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
           Stmt::Return(expr) => {
                let val = self.compile_expression(expr)?;
                // emit dinits for all scopes WITHOUT popping them
                let scopes: Vec<Vec<(String, PointerValue<'ctx>)>> = self.variables.iter()
                    .map(|scope| scope.iter()
                        .map(|(_, slot)| (slot.type_name.clone(), slot.ptr))
                        .collect())
                    .collect();
                for scope_vars in scopes.iter().rev() {
                    self.emit_dinits_for_vars(scope_vars)?;
                }
                self.builder.build_return(Some(&val))
                    .map_err(|e| e.to_string())?;
                Ok(())
            } 


            
            Stmt::Block(stmts) => {
                self.push_scope();
                for s in stmts {
                    self.compile_statement(s)?;
                }
                self.emit_dinits_for_scope()?;
                self.pop_scope();
                Ok(())
            }

            Stmt::Function { name, params, return_type, body } => {
                
                // Build parameter types
                let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                    .map(|(_, ty)| self.type_str_to_llvm(ty).into())
                    .collect();

                let fn_type = match return_type.as_str() {
                    "Int" => self.context.i64_type().fn_type(&param_types, false),
                    "Float" => self.context.f64_type().fn_type(&param_types, false),
                    "Bool" => self.context.bool_type().fn_type(&param_types, false),
                    "Str" => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, false),
                    "Void" => self.context.void_type().fn_type(&param_types, false),
                    other   => self.type_str_to_llvm(other).fn_type(&param_types, false), // default
                };

                let function = self.module.add_function(&name, fn_type, None);
                let entry = self.context.append_basic_block(function, "entry");
                self.builder.position_at_end(entry);

                // Save outer scope, create new one
                let outer_vars = std::mem::take(&mut self.variables);
                self.variables = vec![HashMap::new()];

                // Bind parameters to allocas
                for (i, (param_name, param_type)) in params.iter().enumerate() {
                    let is_ref = param_type.ends_with('&');
                    let ty = if is_ref {
                        self.context.ptr_type(AddressSpace::default()).into()
                    } else {
                       self.type_str_to_llvm(param_type)
                    };
                    let alloca = self.builder.build_alloca(ty, param_name)
                        .map_err(|e| e.to_string())?;
                    let param_val = function.get_nth_param(i as u32)
                        .ok_or_else(|| format!("Missing param {}", i))?;
                    self.builder.build_store(alloca, param_val)
                        .map_err(|e| e.to_string())?;
                    self.define_var(param_name.clone(), VarSlot { ptr: alloca, ty, is_ref, type_name: param_type.clone() });
                }

                // Compile body
                for s in body {
                    self.compile_statement(s)?;
                }
                
                if let Some(last_block) = function.get_last_basic_block() {
                    if last_block.get_terminator().is_none() {
                        self.builder.position_at_end(last_block);
                        
                       let scopes: Vec<Vec<(String, PointerValue<'ctx>)>> = self.variables.iter()
                            .map(|scope| scope.iter()
                                .map(|(_, slot)| (slot.type_name.clone(), slot.ptr))
                                .collect())
                            .collect();
                        for scope_vars in scopes.iter().rev() {
                            self.emit_dinits_for_vars(scope_vars)?;
                        }

                        let ret_type = function.get_type().get_return_type();
                        match ret_type {
                            None => {
                                self.builder.build_return(None)
                                    .map_err(|e| e.to_string())?;
                            }
                            Some(ty) => {
                                let zero = match ty {
                                    BasicTypeEnum::IntType(t)   => t.const_int(0, false).as_basic_value_enum(),
                                    BasicTypeEnum::FloatType(t) => t.const_float(0.0).as_basic_value_enum(),
                                    other => return Err(format!("Cannot emit fallback return for type {:?}", other)),
                                };
                                self.builder.build_return(Some(&zero))
                                    .map_err(|e| e.to_string())?;
                            }
                        }
                    }
                }
                // Restore outer scope
                self.variables = outer_vars;
                Ok(())
            }

            Stmt::If { condition, then_branch, else_branch } => {
                let cond_val = self.compile_expression(condition)?
                    .into_int_value();

                let function = self.builder.get_insert_block()
                    .unwrap().get_parent().unwrap();

                let then_bb = self.context.append_basic_block(function, "then");
                let else_bb = self.context.append_basic_block(function, "else");
                let merge_bb = self.context.append_basic_block(function, "merge");

                self.builder.build_conditional_branch(cond_val, then_bb, else_bb)
                    .map_err(|e| e.to_string())?;

                // Then branch
                self.builder.position_at_end(then_bb);
                self.compile_statement(*then_branch)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb)
                        .map_err(|e| e.to_string())?;
                }

                // Else branch
                self.builder.position_at_end(else_bb);
                if let Some(else_stmt) = else_branch {
                    self.compile_statement(*else_stmt)?;
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb)
                        .map_err(|e| e.to_string())?;
                }

                self.builder.position_at_end(merge_bb);
                Ok(())
            }

            Stmt::While { condition, body } => {
                let function = self.builder.get_insert_block()
                    .unwrap().get_parent().unwrap();

                let cond_bb  = self.context.append_basic_block(function, "while_cond");
                let body_bb  = self.context.append_basic_block(function, "while_body");
                let after_bb = self.context.append_basic_block(function, "while_after");

                self.builder.build_unconditional_branch(cond_bb)
                    .map_err(|e| e.to_string())?;

                // Condition
                self.builder.position_at_end(cond_bb);
                let cond_val = self.compile_expression(condition)?
                    .into_int_value();
                self.builder.build_conditional_branch(cond_val, body_bb, after_bb)
                    .map_err(|e| e.to_string())?;

                // Body
                self.builder.position_at_end(body_bb);
                self.compile_statement(*body)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb)
                        .map_err(|e| e.to_string())?;
                }

                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::Extern { name, params, return_type, variadic } => {
                let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                    .map(|(_, ty)| self.type_str_to_llvm(ty).into()).collect(); 
                let fn_type = match return_type.as_str() {
                    "Int"   => self.context.i64_type().fn_type(&param_types, variadic),
                    "Float" => self.context.f64_type().fn_type(&param_types, variadic),
                    "Bool"  => self.context.bool_type().fn_type(&param_types, variadic),
                    "Str"   => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, variadic),
                    "Ptr"   => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, variadic),
                    "Void"  => self.context.void_type().fn_type(&param_types, variadic),
                    other   => self.type_str_to_llvm(other).fn_type(&param_types, false),
                };
                
                self.module.add_function(&name, fn_type, Some(inkwell::module::Linkage::External));
                Ok(())
            }
            Stmt::Struct { name, fields } => {
                let typed_fields: Vec<(String, LKitType)> = fields.iter()
                    .filter_map(|(n, t)| LKitType::from_str(t).map(|ty| (n.clone(), ty)))
                    .collect();
                self.struct_defs.insert(name.clone(), StructDef { name, fields: typed_fields });
                Ok(())
            }
            
            Stmt::Extend { type_name, items } => {
                let mut compiled = CompiledExtend {
                    init_fn: None,
                    dinit_fn: None,
                    methods: HashMap::new(),
                };

                for item in items {
                    match item {
                        ExtendItem::Init { params, body } => {
                            let init_name = format!("{}__init", type_name);
                            compiled.init_fn = Some(init_name.clone());

                            // init takes params and returns the struct type
                            let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                                .map(|(_, ty)| self.type_str_to_llvm(ty).into())
                                .collect();

                            let struct_ty = self.get_struct_type(&type_name)?;
                            let fn_type = struct_ty.fn_type(&param_types, false);
                            let function = self.module.add_function(&init_name, fn_type, None);
                            let entry = self.context.append_basic_block(function, "entry");
                            self.builder.position_at_end(entry);

                            let outer_vars = std::mem::take(&mut self.variables);
                            self.variables = vec![HashMap::new()];
                            // allocate 'this' as a local struct
                            let this_alloca = self.builder.build_alloca(struct_ty, "this")
                                .map_err(|e| e.to_string())?;
                            self.define_var("this".to_string(), VarSlot {
                                ptr: this_alloca,
                                ty: struct_ty.into(),
                                is_ref: false,
                                type_name: type_name.clone(),
                            });

                            // bind params
                            for (i, (p_name, p_type)) in params.iter().enumerate() {
                                let ty = self.type_str_to_llvm(p_type);
                                let alloca = self.builder.build_alloca(ty, p_name)
                                    .map_err(|e| e.to_string())?;
                                let param_val = function.get_nth_param(i as u32)
                                    .ok_or_else(|| format!("Missing param {}", i))?;
                                self.builder.build_store(alloca, param_val)
                                    .map_err(|e| e.to_string())?;
                                self.define_var(p_name.clone(), VarSlot {
                                    ptr: alloca,
                                    ty,
                                    is_ref: false,
                                    type_name: p_type.clone(),
                                });
                            }

                            // compile body
                            for s in body {
                                self.compile_statement(s)?;
                            }

                            // implicit return this
                            let this_val = self.builder.build_load(
                                BasicTypeEnum::StructType(struct_ty), 
                                this_alloca, 
                                "this_ret"
                            ).map_err(|e| e.to_string())?;
                            self.builder.build_return(Some(&this_val))
                                .map_err(|e| e.to_string())?;

                            self.variables = outer_vars;
                        }

                        ExtendItem::Dinit { body } => {
                            let dinit_name = format!("{}__dinit", type_name);
                            compiled.dinit_fn = Some(dinit_name.clone());

                            // dinit takes a pointer to the struct, returns void
                            let struct_ty = self.get_struct_type(&type_name)?;
                            let ptr_ty = self.context.ptr_type(AddressSpace::default());
                            let fn_type = self.context.void_type().fn_type(&[ptr_ty.into()], false);
                            let function = self.module.add_function(&dinit_name, fn_type, None);
                            let entry = self.context.append_basic_block(function, "entry");
                            self.builder.position_at_end(entry);

                            let outer_vars = std::mem::take(&mut self.variables);
                            self.variables = vec![HashMap::new()];
                            // 'this' is the pointer param
                            let this_ptr = function.get_nth_param(0).unwrap()
                                .into_pointer_value();
                            self.define_var("this".to_string(), VarSlot {
                                ptr: this_ptr,
                                ty: struct_ty.into(),
                                is_ref: false,
                                type_name: type_name.clone(),
                            });

                            for s in body {
                                self.compile_statement(s)?;
                            }

                            self.builder.build_return(None)
                                .map_err(|e| e.to_string())?;

                            self.variables = outer_vars;
                        }

                        ExtendItem::Method { name, params, return_type, body } => {
                            let method_name = format!("{}_{}", type_name, name);
                            compiled.methods.insert(name.clone(), method_name.clone());

                            let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                                .map(|(_, ty)| self.type_str_to_llvm(ty).into())
                                .collect();

                            let ret_ty = match return_type.as_str() {
                                "Int"   => self.context.i64_type().fn_type(&param_types, false),
                                "Float" => self.context.f64_type().fn_type(&param_types, false),
                                "Bool"  => self.context.bool_type().fn_type(&param_types, false),
                                "Str"   => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, false),
                                "Void"  => self.context.void_type().fn_type(&param_types, false),
                                other   => self.type_str_to_llvm(other).fn_type(&param_types, false),
                            };

                            let function = self.module.add_function(&method_name, ret_ty, None);
                            let entry = self.context.append_basic_block(function, "entry");
                            self.builder.position_at_end(entry);

                            let outer_vars = std::mem::take(&mut self.variables);
                            self.variables = vec![HashMap::new()];

                            for (i, (p_name, p_type)) in params.iter().enumerate() {
                                let ty = self.type_str_to_llvm(p_type);
                                let is_handle = p_type.ends_with('&');
                                let alloca = self.builder.build_alloca(ty, p_name)
                                    .map_err(|e| e.to_string())?;
                                let param_val = function.get_nth_param(i as u32)
                                    .ok_or_else(|| format!("Missing param {}", i))?;
                                self.builder.build_store(alloca, param_val)
                                    .map_err(|e| e.to_string())?;
                                self.define_var(p_name.clone(), VarSlot {
                                    ptr: alloca,
                                    ty,
                                    is_ref: is_handle,
                                    type_name: p_type.clone(),
                                });
                            }

                            for s in body {
                                self.compile_statement(s)?;
                            }

                            // if void, emit return
                            if return_type == "Void" {
                                if self.builder.get_insert_block()
                                    .unwrap().get_terminator().is_none() {
                                    self.builder.build_return(None)
                                        .map_err(|e| e.to_string())?;
                                }
                            }

                            self.variables = outer_vars;
                        }
                    }
                }

                self.extends.insert(type_name, compiled);
                Ok(())
            }

            Stmt::LetDecl { .. } => unreachable!("LetDecl should be folded by type checker"),
            Stmt::Import { .. } => Ok(()),
            Stmt::Tool { .. } => Ok(()),  // no codegen needed

            Stmt::ExtendWith { type_name, tool_name: _, items } => {
                // compile exactly like Stmt::Extend methods
                // reuse the same extend compilation logic
                let mut compiled = self.extends.get(&type_name).cloned()
                    .unwrap_or(CompiledExtend {
                        init_fn: None,
                        dinit_fn: None,
                        methods: HashMap::new(),
                    });

                for item in items {
                    if let ExtendItem::Method { name, params, return_type, body } = item {
                        let method_name = format!("{}_{}", type_name, name);
                        compiled.methods.insert(name.clone(), method_name.clone());

                        let param_types: Vec<BasicMetadataTypeEnum> = params.iter()
                            .map(|(_, ty)| self.type_str_to_llvm(ty).into())
                            .collect();

                        let ret_ty = match return_type.as_str() {
                            "Int"   => self.context.i64_type().fn_type(&param_types, false),
                            "Float" => self.context.f64_type().fn_type(&param_types, false),
                            "Bool"  => self.context.bool_type().fn_type(&param_types, false),
                            "Str"   => self.context.ptr_type(AddressSpace::default()).fn_type(&param_types, false),
                            "Void"  => self.context.void_type().fn_type(&param_types, false),
                            other   => self.type_str_to_llvm(other).fn_type(&param_types, false),
                        };

                        let function = self.module.add_function(&method_name, ret_ty, None);
                        let entry = self.context.append_basic_block(function, "entry");
                        self.builder.position_at_end(entry);

                        let outer_vars = std::mem::take(&mut self.variables);
                        self.variables = vec![HashMap::new()];

                        for (i, (p_name, p_type)) in params.iter().enumerate() {
                            let ty = self.type_str_to_llvm(p_type);
                            let is_ref = p_type.ends_with('&');
                            let alloca = self.builder.build_alloca(ty, p_name)
                                .map_err(|e| e.to_string())?;
                            let param_val = function.get_nth_param(i as u32)
                                .ok_or_else(|| format!("Missing param {}", i))?;
                            self.builder.build_store(alloca, param_val)
                                .map_err(|e| e.to_string())?;
                            self.define_var(p_name.clone(), VarSlot {
                                ptr: alloca,
                                ty,
                                is_ref,
                                type_name: p_type.clone(),
                            });
                        }

                        for s in body {
                            self.compile_statement(s)?;
                        }

                        if return_type == "Void" {
                            if self.builder.get_insert_block()
                                .unwrap().get_terminator().is_none() {
                                self.builder.build_return(None)
                                    .map_err(|e| e.to_string())?;
                            }
                        }

                        self.variables = outer_vars;
                    }
                }

                self.extends.insert(type_name, compiled);
                Ok(())
            }

        }
    }
    
    
    // when compiling module stmts, prefix function names:
    pub fn compile_module(&mut self, module_name: &str, stmts: Vec<Stmt>) -> Result<(), String> {
        for stmt in stmts {
            match stmt {
                Stmt::Function { name, params, return_type, body } => {
                    let mangled = format!("{}__{}", module_name, name);
                    self.compile_statement(Stmt::Function {
                        name: mangled,
                        params,
                        return_type,
                        body,
                    })?;
                }
                other => self.compile_statement(other)?,
            }
        }
        Ok(())
    }

    fn type_str_to_llvm(&self, ty: &str) -> BasicTypeEnum<'ctx> {
        match ty {
            "Int" => self.context.i64_type().into(),
            "Float" => self.context.f64_type().into(),
            "Bool" => self.context.bool_type().into(),
            "Str" => self.context.ptr_type(AddressSpace::default()).into(),
            "Ptr" => self.context.ptr_type(AddressSpace::default()).into(),
            "Byte" => self.context.i8_type().into(),
            other if other.ends_with('&') => {
                self.context.ptr_type(AddressSpace::default()).into()
            } 
            other if other.ends_with('*') && !other.ends_with("strict&") => {
                self.context.ptr_type(AddressSpace::default()).into()
            }
            other if other.starts_with('[') => {
                // [T] — dynamic slice: { ptr, i64 len, i64 cap }
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.ptr_type(AddressSpace::default());
                self.context.struct_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false).into()
            }
            other if other.contains('[') => {
                // T[N] — fixed slice
                if let Some(bracket) = other.find('[') {
                    let base = &other[..bracket];
                    let n: u32 = other[bracket+1..other.len()-1].parse().unwrap_or(0);
                    let elem_ty = self.type_str_to_llvm(base);
                    return elem_ty.array_type(n).into();
                }
                self.context.i64_type().into()
            }

            other   => {
                // try to resolve as a struct
                if let Some(def) = self.struct_defs.get(other) {
                    let field_types: Vec<BasicTypeEnum> = def.fields.iter()
                        .map(|(_, t)| self.type_str_to_llvm(&t.to_str()))
                        .collect();
                    self.context.struct_type(&field_types, false).into()
                } else {
                    // fallback — should never hit if type checker did its job
                    eprintln!("WARNING: unknown type '{}', defaulting to i64", other);
                    self.context.i64_type().into()
                }
            }
         }
    }
    
    fn compile_lvalue(&self, expr: &Expr) -> Result<(PointerValue<'ctx>, BasicTypeEnum<'ctx>), String> {
        match expr {
            Expr::Variable(name) => {
                let slot = self.lookup_var(name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                if slot.is_ref {
                    // deref the handle to get the pointer it points to
                    let handle_val = self.builder.build_load(slot.ty, slot.ptr, name)
                        .map_err(|e| e.to_string())?;
                    Ok((handle_val.into_pointer_value(), slot.ty))
                } else if slot.type_name.ends_with('*') {
                    // heap owner — write through pointer
                    let val = self.builder.build_load(slot.ty, slot.ptr, &name)
                        .map_err(|e| e.to_string())?;
                    let pointee_name = slot.type_name.trim_end_matches('*').trim().to_string();
                    let pointee_ty = self.type_str_to_llvm(&pointee_name);
                    Ok((val.into_pointer_value(), pointee_ty))
                } else {
                    Ok((slot.ptr, slot.ty))
                }
            }
          
            Expr::FieldAccess { object, field } => {
                let var_name = match object.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => return Err("Complex field assignment not yet supported".into()),
                };
                let slot = self.lookup_var(&var_name)
                    .ok_or_else(|| format!("Undefined variable '{}'", var_name))?;

                // deref handle if needed
                let struct_ptr = if slot.is_ref || slot.type_name.ends_with('*') {
                    let handle_val = self.builder.build_load(
                        self.context.ptr_type(AddressSpace::default()),
                        slot.ptr,
                        &var_name
                    ).map_err(|e| e.to_string())?;
                    handle_val.into_pointer_value()
                } else {
                    slot.ptr
                };

                // strip handle wrapper from type name: "Point strict&" -> "Point"
                let struct_name = slot.type_name
                    .trim_end_matches('*')
                    .trim_end_matches('&')
                    .trim_end_matches("strict")
                    .trim()
                    .to_string();

                let def = self.struct_defs.get(&struct_name).cloned()
                    .ok_or_else(|| format!("Unknown struct '{}'", struct_name))?;
                let idx = def.field_index(field)
                    .ok_or_else(|| format!("No field '{}' on '{}'", field, struct_name))? as u32;
                let struct_ty = self.get_struct_type(&struct_name)?;
                let field_ty = self.type_str_to_llvm(&def.fields[idx as usize].1.to_str());
                let ptr = self.builder.build_struct_gep(struct_ty, struct_ptr, idx, field)
                    .map_err(|e| e.to_string())?;
                Ok((ptr, field_ty))
            }

            Expr::Index { object, index } => {
                let name = match object.as_ref() {
                    Expr::Variable(n) => n.clone(),
                    _ => return Err("Complex index assignment not yet supported".into()),
                };
                let slot = self.lookup_var(&name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                let idx_val = self.compile_expression(*index.clone())?.into_int_value();
                match slot.ty {
                    BasicTypeEnum::ArrayType(arr_ty) => {
                        let elem_ty = arr_ty.get_element_type();
                        let ptr = unsafe {
                            self.builder.build_gep(slot.ty, slot.ptr, &[
                                self.context.i64_type().const_int(0, false),
                                idx_val,
                            ], "elem_ptr").map_err(|e| e.to_string())?
                        };
                        Ok((ptr, elem_ty))
                    }
                    _ => Err(format!("Cannot index-assign into non-array '{}'", name)),
                }
            }
            _ => Err("Invalid lvalue".into()),
        }
    }


    // Helper to get LLVM struct type:
    fn get_struct_type(&self, name: &str) -> Result<inkwell::types::StructType<'ctx>, String> {
       let base_name = name
           .trim_end_matches('*')
           .trim_end_matches('&')
           .trim_end_matches("strict")
           .trim();
        let def = self.struct_defs.get(base_name)
            .ok_or_else(|| format!("Unknown struct '{}'", base_name))?;
        let field_types: Vec<BasicTypeEnum> = def.fields.iter()
            .map(|(_, ty)| self.type_str_to_llvm(&ty.to_str()))
            .collect();
        Ok(self.context.struct_type(&field_types, false))
    }

     // scope helpers
    fn push_scope(&mut self) {
        self.variables.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.variables.pop();
    }

    fn define_var(&mut self, name: String, slot: VarSlot<'ctx>) {
        self.variables.last_mut().unwrap().insert(name, slot);
    }

    fn lookup_var(&self, name: &str) -> Option<&VarSlot<'ctx>> {
        for scope in self.variables.iter().rev() {
            if let Some(slot) = scope.get(name) {
                return Some(slot);
            }
        }
        None
    }


    fn emit_dinits_for_vars(&mut self, vars: &[(String, PointerValue<'ctx>)]) -> Result<(), String> {
        for (type_name, ptr) in vars {
            let is_heap = type_name.ends_with('*');
            let struct_name = type_name
                .trim_end_matches('*')
                .trim_end_matches('&')
                .trim_end_matches("strict")
                .trim()
                .to_string();
            let heap_ptr = if is_heap {
                Some(self.builder.build_load(
                    self.context.ptr_type(AddressSpace::default()),
                    *ptr,
                    "heap_ptr"
                ).map_err(|e| e.to_string())?.into_pointer_value())
            } else {
                None
            };
            if let Some(extend) = self.extends.get(&struct_name).cloned() {
                if let Some(dinit_name) = extend.dinit_fn {
                    if let Some(dinit_fn) = self.module.get_function(&dinit_name) {
                        let target = heap_ptr.unwrap_or(*ptr);
                        self.builder.build_call(dinit_fn, &[target.into()], "")
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            if is_heap {
                let free_fn = self.module.get_function("free").unwrap_or_else(|| {
                    let ty = self.context.void_type().fn_type(
                        &[self.context.ptr_type(AddressSpace::default()).into()], false
                    );
                    self.module.add_function("free", ty, Some(inkwell::module::Linkage::External))
                });
                self.builder.build_call(free_fn, &[heap_ptr.unwrap().into()], "")
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn emit_dinits_for_scope(&mut self) -> Result<(), String> {
        let vars: Vec<(String, PointerValue<'ctx>)> = match self.variables.last() {
            Some(scope) => scope.iter()
                .map(|(_, slot)| (slot.type_name.clone(), slot.ptr))
                .collect(),
            None => return Ok(()),
        };
        self.emit_dinits_for_vars(&vars)
    }

        
  
}
