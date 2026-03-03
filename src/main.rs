mod lexer;
mod ast;
mod value;
mod parser;
mod compiler;

use lexer::Lexer;
use lexer::TokenType;
use parser::Parser;
use compiler::Compiler;
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::OptimizationLevel;

fn lex(source: &str) -> Vec<TokenType> {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    loop {
        let tok = lexer.next_token();
        let is_eof = tok == TokenType::EOF;
        tokens.push(tok);
        if is_eof { break; }
    }
    tokens
}

use std::env;
use std::fs;

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

    let source = fs::read_to_string(path)
        .unwrap_or_else(|e| {
            eprintln!("Error reading file '{}': {}", path, e);
            std::process::exit(1);
        });

    // Lex
    let tokens = lex(&source);

    // Parse
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse();

    // Compile
    let context = Context::create();
    let mut compiler = Compiler::new(&context, "rkit");

    for stmt in stmts {
        compiler.compile_statement(stmt).unwrap_or_else(|e| {
            eprintln!("Compile error: {}", e);
            std::process::exit(1);
        });
    }

    compiler.module.print_to_stderr();

    // Output filename: strip .lk, add .o
    let output_name = path.strip_suffix(".lk").unwrap_or(path);
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

    machine
        .write_to_file(&compiler.module, FileType::Object, obj_path.as_ref())
        .expect("Failed to write object file");

    println!("Compiled to {}", obj_path);
    println!("Link with: gcc {} -o {}", obj_path, output_name);
}
