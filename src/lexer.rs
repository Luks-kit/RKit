use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Keywords
    Fn, Int, Str, Float, Bool, 
    Return, If, Else, While,
    
    // Literals & Identifiers
    Identifier(String),
    Literal(Value),
    
    // Operators & Punctuation
    Plus, Minus, Star, Slash, Not, Equal,
    EqualEqual, NotEqual,
    Less, Greater, LessEqual, GreaterEqual,
    LParen, RParen, LBrace, RBrace, 
    Semicolon, Comma,
    
    EOF,
}

use std::iter::Peekable;
use std::str::Chars;

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
        }
    }

    // Helper to consume whitespace
    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.input.peek() {
            if c.is_whitespace() {
                self.input.next();
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> TokenType {
        self.skip_whitespace();

        let c = match self.input.next() {
            Some(c) => c,
            None => return TokenType::EOF,
        };

        match c {
            '(' => TokenType::LParen,
            ')' => TokenType::RParen,
            '{' => TokenType::LBrace,
            '}' => TokenType::RBrace,
            ';' => TokenType::Semicolon,
            ',' => TokenType::Comma,
            '+' => TokenType::Plus,
            '-' => TokenType::Minus,
            '*' => TokenType::Star,
            '/' => TokenType::Slash,            
            '=' => {
                if self.input.peek() == Some(&'=') {
                    self.input.next();
                    TokenType::EqualEqual
                } else {
                    TokenType::Equal
                }
            }
            '!' => {
                if self.input.peek() == Some(&'=') {
                    self.input.next();
                    TokenType::NotEqual
                } else {
                    // For now, if it's just '!', we'll treat as error or add Not later
                    TokenType::Not 
                }
            }
            '<' => {
                if self.input.peek() == Some(&'=') {
                    self.input.next();
                    TokenType::LessEqual
                } else {
                    TokenType::Less
                }
            }
            '>' => {
                if self.input.peek() == Some(&'=') {
                    self.input.next();
                    TokenType::GreaterEqual
                } else {
                    TokenType::Greater
                }
            }

            '"' => self.read_string(),
            _ if c.is_ascii_digit() => self.read_number(c),
            _ if c.is_alphabetic() => self.read_identifier(c),
            _ => TokenType::EOF, // Should probably be an Error type later
        }
    }
}

impl<'a> Lexer<'a> {
    fn read_identifier(&mut self, first_char: char) -> TokenType {
        let mut ident = String::from(first_char);
        while let Some(&c) = self.input.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(self.input.next().unwrap());
            } else {
                break;
            }
        }

        // Match keywords
        match ident.as_str() {
            "fn" => TokenType::Fn,
            "int" => TokenType::Int,
            "str" => TokenType::Str,
            "float" => TokenType::Float,
            "bool" => TokenType::Bool,
            "if" => TokenType::If,
            "else" => TokenType::Else,
            "while" => TokenType::While,
            "return" => TokenType::Return,
            "true" => TokenType::Literal(Value::Bool(true)),
            "false" => TokenType::Literal(Value::Bool(false)),
            _ => TokenType::Identifier(ident),
        }
    }

    fn read_number(&mut self, first_char: char) -> TokenType {
        let mut number = String::from(first_char);
        while let Some(&c) = self.input.peek() {
            if c.is_ascii_digit() {
                number.push(self.input.next().unwrap());
            } else { break; }
        }
        let val = number.parse::<i64>().unwrap_or(0);
        TokenType::Literal(Value::Int(val))     
    }
    
    fn read_string(&mut self) -> TokenType {
        let mut s = String::new();
        while let Some(c) = self.input.next() {
            if c == '"' { break; }
            s.push(c);
        }
        TokenType::Literal(Value::Str(s))
    }
}
