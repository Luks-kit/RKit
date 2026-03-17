mod ast;
mod compiler;
mod lexer;
mod parser;
mod typechecker;
mod types;
mod value;

use ast::Stmt;
use compiler::Compiler;
use inkwell::OptimizationLevel;
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use lexer::TokenType;
use lexer::{Lexer, Token};
use parser::Parser;
use std::collections::{HashMap, HashSet};
use typechecker::TypeChecker;

fn lex(source: &str, file: &str) -> Vec<Token> {
    let mut lexer = Lexer::new(source, file);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let is_eof = tok.kind == TokenType::EOF;
        tokens.push(tok);
        if is_eof {
            break;
        }
    }
    tokens
}

use std::env;
use std::fs;

fn load_module(name: &str, search_paths: &[&str], loaded: &mut HashSet<String>) -> Vec<Stmt> {
    if loaded.contains(name) {
        return vec![]; // already loaded
    }
    loaded.insert(name.to_string());

    // find the file
    let filename = format!("{}.lk", name);
    let path = search_paths
        .iter()
        .map(|p| std::path::Path::new(p).join(&filename))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("Cannot find module '{}'", name));

    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("Cannot read module '{}'", name));

    let path_str = path.to_string_lossy();
    let tokens = lex(&source, &path_str);
    let stmts = Parser::new(tokens).parse();

    // tag all top-level declarations with module name
    // (we'll handle this via the module registry, not AST mutation)
    stmts
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: lkit <file.lk>");
        std::process::exit(1);
    }

    let path = &args[1];
    if !path.ends_with(".lk") {
        eprintln!("Warning: expected a .lk file");
    }

    let source = fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Error reading file '{}': {}", path, e);
        std::process::exit(1);
    });

    // Lex
    let tokens = lex(&source, path);

    // Parse
    let mut parser = Parser::new(tokens);
    let stmts_unchecked = parser.parse();

    // resolve imports
    let mut loaded = HashSet::new();
    let mut all_modules: HashMap<String, Vec<Stmt>> = HashMap::new();

    for stmt in &stmts_unchecked {
        if let Stmt::Import { module_name } = stmt {
            let module_stmts = load_module(
                module_name.as_str(),
                &[".", "/usr/local/lib/lkit"], // search paths
                &mut loaded,
            );
            all_modules.insert(module_name.clone(), module_stmts);
        }
    }

    let mut checker = TypeChecker::new();

    for (mod_name, mod_stmts) in &all_modules {
        checker.register_module(mod_name, mod_stmts);
    }

    checker.register_pass(&stmts_unchecked);

    checker.check(&stmts_unchecked);

    if !checker.errors.is_empty() {
        for err in &checker.errors {
            if let Some(span) = &err.span {
                eprintln!(
                    "{}:{}:{}: Type error: {}",
                    span.file, span.line, span.col, err.message
                );
            } else {
                eprintln!("Type error: {}", err.message);
            }
        }
        std::process::exit(1);
    }

    let stmts = checker.transform(stmts_unchecked);

    // Compile
    let context = Context::create();
    let mut compiler = Compiler::new(&context, "rkit");

    // compile modules first
    for (mod_name, mod_stmts) in &all_modules {
        compiler.modules.insert(mod_name.clone());
        if let Result::Err(err) = compiler.compile_module(mod_name, mod_stmts.clone()) {
            eprintln!("Error compiling module {}: {}", mod_name, err);
            std::process::exit(1);
        }
    }

    // then compile main file
    for stmt in stmts {
        compiler.compile_statement(stmt).unwrap_or_else(|e| {
            eprintln!("Compile error: {}", e);
            std::process::exit(1);
        });
    }

    let output_name = path.strip_suffix(".lk").unwrap_or(path);
    let ir_path = format!("{}.ll", output_name);
    let _ = compiler.module.print_to_file(ir_path);

    // Output filename: strip .lk, add .o
    let obj_path = format!("{}.o", output_name);

    Target::initialize_native(&InitializationConfig::default())
        .expect("Failed to initialize native target");

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).expect("Failed to get target");
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .expect("Failed to create target machine");

    if let Err(err) = compiler.module.verify() {
        eprintln!("LLVM verification error: {}", err.to_string());
        std::process::exit(1);
    }

    machine
        .write_to_file(&compiler.module, FileType::Object, obj_path.as_ref())
        .expect("Failed to write object file");

    println!("Compiled to {}", obj_path);
    println!("Link with: gcc {} -o {}", obj_path, output_name);
}
