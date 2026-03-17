use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenType {
    // Keywords
    Fn, Int, Str, Float, Bool, Void, Ptr, Byte,
    Return, If, Else, While,
    Extern, Struct, Len, Cast, 
    Extend, Init, Dinit,
    Strict, Import,
    Tool,
    With,
    // Literals & Identifiers
    Identifier(String),
    Literal(Value),
    
    // Operators & Punctuation
    Plus, Minus, Star, Slash, Not, Equal,
    EqualEqual, NotEqual,
    Amp, Pipe, AmpAmp, PipePipe,
    Less, Greater, LessEqual, GreaterEqual,
    LParen, RParen, 
    LBrace, RBrace, LBracket, RBracket,
    Semicolon, Colon, Comma, Dot, Variadic,
    
    EOF,
}

use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(file: &str, line: usize, col: usize) -> Self {
        Span { file: file.to_string(), line, col }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenType,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenType, span: Span) -> Self {
        Self { kind, span }
    }
}

pub struct Lexer<'a> {
    input: Peekable<Chars<'a>>,
    line: usize,
    col: usize,
    file: String,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str, file: &'a str) -> Self {
        Self {
            input: source.chars().peekable(),
            line: 1,
            col: 1,
            file: file.to_string() 
        }
    }

    // Helper to consume whitespace
    fn skip_whitespace(&mut self) {
        while let Some(&c) = self.input.peek() {
            if c == '\n' { self.line += 1; self.col += 1; }
            if c.is_whitespace() {
                self.input.next(); self.col += 1;
            } else {
                break;
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_whitespace();
        let start_col = self.col;
        let start_line = self.line;

        let c = match self.input.next() {
            Some(c) => c,
            None => return Token::new(
                TokenType::EOF, 
                Span::new(&self.file, start_line, start_col)),
        };

        let kind = match c {
            '\n' => { self.line += 1; return self.next_token(); }
            '(' => TokenType::LParen,
            ')' => TokenType::RParen,
            '{' => TokenType::LBrace,
            '}' => TokenType::RBrace,
            ';' => TokenType::Semicolon,
            ',' => TokenType::Comma,
            '+' => TokenType::Plus,
            '-' => TokenType::Minus,
            '*' => TokenType::Star,          
            ':' => TokenType::Colon,
            '[' => TokenType::LBracket,
            ']' => TokenType::RBracket,
            '/' => {
                if self.input.peek() == Some(&'/') {
                    while let Some(&c) = self.input.peek() {
                        if c == '\n' { break; }
                        self.input.next();
                    }
                    return self.next_token();
                } else if self.input.peek() == Some(&'*') {
                    self.input.next(); // consume '*'
                    loop {
                        match self.input.next() {
                            Some('*') if self.input.peek() == Some(&'/') => {
                                self.input.next(); // consume '/'
                                break;
                            }
                            Some('\n') => { self.line += 1; }
                            None => panic!("Unterminated block comment"),
                            _ => {}
                        }
                    }
                    return self.next_token();
                } else {
                    TokenType::Slash
                }
            }
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
            '.' => {
                if self.input.peek() == Some(&'.') {
                    self.input.next();
                    if self.input.peek() == Some(&'.') {
                        self.input.next();
                        TokenType::Variadic
                    } else {
                        panic!("Unexpected '..' — did you mean '...'?");
                    }
                } else {
                    TokenType::Dot
                }
            }
            '&' => {
                if self.input.peek() == Some(&'&') {
                    self.input.next();
                    TokenType::AmpAmp
                } else {
                    TokenType::Amp
                }
            }
            '|' => {
                if self.input.peek() == Some(&'|') {
                    self.input.next();
                    TokenType::PipePipe
                } else {
                    TokenType::Pipe
                }
            }

            '"' => self.read_string(),
            _ if c.is_ascii_digit() => self.read_number(c),
            _ if c.is_alphabetic() => self.read_identifier(c),
            _ => TokenType::EOF, // Should probably be an Error type later
        };
        Token::new(kind, Span::new(&self.file, start_line, start_col))
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
            "extern" => TokenType::Extern,
            "int" => TokenType::Int,
            "str" => TokenType::Str,
            "float" => TokenType::Float,
            "bool" => TokenType::Bool,
            "void" => TokenType::Void,
            "ptr" => TokenType::Ptr,
            "byte" => TokenType::Byte,
            "struct" => TokenType::Struct,
            "strict" => TokenType::Strict,
            "tool" => TokenType::Tool,
            "with" => TokenType::With,
            "import" => TokenType::Import,
            "cast" => TokenType::Cast,
            "extend" => TokenType::Extend,
            "init"   => TokenType::Init,
            "dinit"  => TokenType::Dinit,
            "if" => TokenType::If,
            "else" => TokenType::Else,
            "while" => TokenType::While,
            "return" => TokenType::Return,
            "len" => TokenType::Len,
            "true" => TokenType::Literal(Value::Bool(true)),
            "false" => TokenType::Literal(Value::Bool(false)),
            _ => TokenType::Identifier(ident),
        }
    }

    fn read_number(&mut self, first_char: char) -> TokenType {
        let mut number = String::from(first_char);
        let mut is_float = false;
        
        while let Some(&c) = self.input.peek() {
            if c.is_ascii_digit() {
                number.push(self.input.next().unwrap());
            } else if c == '.' && !is_float {
                is_float = true;
                number.push(self.input.next().unwrap());
            } else {
                break;
            }
        }
        
        if is_float {
            let val = number.parse::<f64>().unwrap_or(0.0);
            TokenType::Literal(Value::Float(val))
        } else {
            let val = number.parse::<i64>().unwrap_or(0);
            TokenType::Literal(Value::Int(val))
        }
    }

    fn read_string(&mut self) -> TokenType {
        let mut s = String::new();
        while let Some(c) = self.input.next() {
            if c == '"' { break; }
            if c == '\\' {
                match self.input.next() {
                    Some('n')  => s.push('\n'),
                    Some('t')  => s.push('\t'),
                    Some('r')  => s.push('\r'),
                    Some('\\') => s.push('\\'),
                    Some('"')  => s.push('"'),
                    Some(c)    => { s.push('\\'); s.push(c); }
                    None       => break,
                }
            } else {
                s.push(c);
            }
        } 
        TokenType::Literal(Value::Str(s))
    }
}
