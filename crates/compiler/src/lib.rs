//! Compiler for the compact privacy-contract language used by PPSC.
//!
//! The compiler deliberately emits a backend-neutral operator DAG. Runtime
//! committees decide how FHE and secret-sharing nodes are evaluated.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use tiny_keccak::{Hasher, Keccak};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub message: String,
    pub offset: usize,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for CompileError {}

type Result<T> = std::result::Result<T, CompileError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueType {
    Address,
    Uint,
    FheUint,
    FheBool,
    Sint,
    SecretBool,
    /// Result of an explicitly authorized Pick opening.
    Opened,
    Void,
}

impl ValueType {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "address" => Some(Self::Address),
            "uint" | "uint256" => Some(Self::Uint),
            "FheUint" => Some(Self::FheUint),
            "Sint" => Some(Self::Sint),
            "SecretBool" => Some(Self::SecretBool),
            _ => None,
        }
    }

    fn abi_type(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::Uint => "uint256",
            Self::FheUint | Self::FheBool | Self::Sint | Self::SecretBool | Self::Opened => {
                "bytes32"
            }
            Self::Void => "",
        }
    }

    fn representation(self) -> Representation {
        match self {
            Self::FheUint | Self::FheBool => Representation::Fhe,
            Self::Sint | Self::SecretBool => Representation::SecretSharing,
            _ => Representation::Public,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    Public,
    Fhe,
    SecretSharing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TokenKind {
    Ident(String),
    Number(String),
    Symbol(char),
    Eof,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    offset: usize,
}

fn lex(source: &str) -> Result<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && bytes.get(i + 1) == Some(&b'/') {
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let next = bytes[i] as char;
                if !(next.is_ascii_alphanumeric() || next == '_' || next == '.') {
                    break;
                }
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Ident(source[start..i].to_owned()),
                offset: start,
            });
        } else if c.is_ascii_digit() {
            let start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Number(source[start..i].to_owned()),
                offset: start,
            });
        } else if "{}()[];,=".contains(c) {
            tokens.push(Token {
                kind: TokenKind::Symbol(c),
                offset: i,
            });
            i += 1;
        } else {
            return Err(CompileError {
                message: format!("unexpected character `{c}`"),
                offset: i,
            });
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        offset: source.len(),
    });
    Ok(tokens)
}

#[derive(Debug, Clone)]
struct Program {
    name: String,
    states: Vec<StateDecl>,
    functions: Vec<Function>,
}

#[derive(Debug, Clone)]
struct StateDecl {
    name: String,
    ty: ValueType,
    key_ty: Option<ValueType>,
}

#[derive(Debug, Clone)]
struct Function {
    name: String,
    params: Vec<Param>,
    body: Vec<Statement>,
}

#[derive(Debug, Clone)]
struct Param {
    name: String,
    ty: ValueType,
}

#[derive(Debug, Clone)]
enum Statement {
    Local(ValueType, String, Expression),
    Assign(Expression, Expression),
    Require(Expression),
    Return(Expression),
    Expression(Expression),
}

#[derive(Debug, Clone)]
enum Expression {
    Name(String),
    Number(String),
    Index(String, Box<Expression>),
    Call(String, Vec<Expression>),
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn new(source: &str) -> Result<Self> {
        Ok(Self {
            tokens: lex(source)?,
            cursor: 0,
        })
    }

    fn parse(mut self) -> Result<Program> {
        self.keyword("privacy")?;
        self.keyword("contract")?;
        let name = self.ident()?;
        self.symbol('{')?;
        let mut states = Vec::new();
        let mut functions = Vec::new();
        while !self.consume_symbol('}') {
            if self.consume_keyword("private") {
                states.push(self.state_decl()?);
            } else if self.consume_keyword("function") {
                functions.push(self.function()?);
            } else {
                return self.fail("expected `private` or `function`");
            }
        }
        if !matches!(self.current().kind, TokenKind::Eof) {
            return self.fail("unexpected input after contract");
        }
        Ok(Program {
            name,
            states,
            functions,
        })
    }

    fn state_decl(&mut self) -> Result<StateDecl> {
        let ty = self.value_type()?;
        let name = self.ident()?;
        let key_ty = if self.consume_symbol('[') {
            let key = self.value_type()?;
            self.symbol(']')?;
            Some(key)
        } else {
            None
        };
        self.symbol(';')?;
        Ok(StateDecl { name, ty, key_ty })
    }

    fn function(&mut self) -> Result<Function> {
        let name = self.ident()?;
        self.symbol('(')?;
        let mut params = Vec::new();
        if !self.consume_symbol(')') {
            loop {
                let ty = self.value_type()?;
                let param_name = self.ident()?;
                params.push(Param {
                    name: param_name,
                    ty,
                });
                if self.consume_symbol(')') {
                    break;
                }
                self.symbol(',')?;
            }
        }
        self.keyword("public")?;
        self.symbol('{')?;
        let mut body = Vec::new();
        while !self.consume_symbol('}') {
            body.push(self.statement()?);
        }
        Ok(Function { name, params, body })
    }

    fn statement(&mut self) -> Result<Statement> {
        if self.consume_keyword("require") {
            self.symbol('(')?;
            let value = self.expression()?;
            self.symbol(')')?;
            self.symbol(';')?;
            return Ok(Statement::Require(value));
        }
        if self.consume_keyword("return") {
            let value = self.expression()?;
            self.symbol(';')?;
            return Ok(Statement::Return(value));
        }
        if let TokenKind::Ident(type_name) = &self.current().kind {
            if let Some(ty) = ValueType::parse(type_name) {
                self.cursor += 1;
                let name = self.ident()?;
                self.symbol('=')?;
                let value = self.expression()?;
                self.symbol(';')?;
                return Ok(Statement::Local(ty, name, value));
            }
        }
        let left = self.expression()?;
        if self.consume_symbol('=') {
            let right = self.expression()?;
            self.symbol(';')?;
            Ok(Statement::Assign(left, right))
        } else {
            self.symbol(';')?;
            Ok(Statement::Expression(left))
        }
    }

    fn expression(&mut self) -> Result<Expression> {
        match self.current().kind.clone() {
            TokenKind::Number(value) => {
                self.cursor += 1;
                Ok(Expression::Number(value))
            }
            TokenKind::Ident(name) => {
                self.cursor += 1;
                if self.consume_symbol('(') {
                    let mut args = Vec::new();
                    if !self.consume_symbol(')') {
                        loop {
                            args.push(self.expression()?);
                            if self.consume_symbol(')') {
                                break;
                            }
                            self.symbol(',')?;
                        }
                    }
                    Ok(Expression::Call(name, args))
                } else if self.consume_symbol('[') {
                    let key = self.expression()?;
                    self.symbol(']')?;
                    Ok(Expression::Index(name, Box::new(key)))
                } else {
                    Ok(Expression::Name(name))
                }
            }
            _ => self.fail("expected expression"),
        }
    }

    fn value_type(&mut self) -> Result<ValueType> {
        let offset = self.current().offset;
        let name = self.ident()?;
        ValueType::parse(&name).ok_or_else(|| CompileError {
            message: format!("unknown type `{name}`"),
            offset,
        })
    }

    fn keyword(&mut self, expected: &str) -> Result<()> {
        if self.consume_keyword(expected) {
            Ok(())
        } else {
            self.fail(&format!("expected `{expected}`"))
        }
    }

    fn consume_keyword(&mut self, expected: &str) -> bool {
        if matches!(&self.current().kind, TokenKind::Ident(value) if value == expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> Result<String> {
        if let TokenKind::Ident(value) = self.current().kind.clone() {
            self.cursor += 1;
            Ok(value)
        } else {
            self.fail("expected identifier")
        }
    }

    fn symbol(&mut self, expected: char) -> Result<()> {
        if self.consume_symbol(expected) {
            Ok(())
        } else {
            self.fail(&format!("expected `{expected}`"))
        }
    }

    fn consume_symbol(&mut self, expected: char) -> bool {
        if self.current().kind == TokenKind::Symbol(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn fail<T>(&self, message: &str) -> Result<T> {
        Err(CompileError {
            message: message.to_owned(),
            offset: self.current().offset,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractManifest {
    pub language_version: u32,
    pub contract: String,
    pub state: Vec<StateManifest>,
    pub functions: Vec<FunctionManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateManifest {
    pub name: String,
    pub value_type: ValueType,
    pub key_type: Option<ValueType>,
    pub representation: Representation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionManifest {
    pub name: String,
    pub signature: String,
    pub selector: String,
    pub inputs: Vec<AbiInput>,
    pub operators: Vec<OperatorNode>,
    pub state_writes: Vec<StateWrite>,
    pub opening_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiInput {
    pub name: String,
    pub source_type: ValueType,
    pub abi_type: String,
    pub representation: Representation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorNode {
    pub id: String,
    pub opcode: String,
    pub domain: Representation,
    pub inputs: Vec<String>,
    pub output_type: ValueType,
    pub representation: Representation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateWrite {
    pub state: String,
    pub key: Option<String>,
    pub value: String,
}

#[derive(Debug, Clone)]
struct TypedRef {
    ty: ValueType,
    reference: String,
}

struct Lowerer<'a> {
    program: &'a Program,
    function: &'a Function,
    env: HashMap<String, TypedRef>,
    operators: Vec<OperatorNode>,
    writes: Vec<StateWrite>,
    opening_output: Option<String>,
}

impl<'a> Lowerer<'a> {
    fn new(program: &'a Program, function: &'a Function) -> Self {
        let mut env = HashMap::new();
        env.insert(
            "msg.sender".to_owned(),
            TypedRef {
                ty: ValueType::Address,
                reference: "context:msg.sender".to_owned(),
            },
        );
        for param in &function.params {
            env.insert(
                param.name.clone(),
                TypedRef {
                    ty: param.ty,
                    reference: format!("input:{}", param.name),
                },
            );
        }
        Self {
            program,
            function,
            env,
            operators: Vec::new(),
            writes: Vec::new(),
            opening_output: None,
        }
    }

    fn lower(mut self) -> Result<FunctionManifest> {
        for statement in &self.function.body {
            match statement {
                Statement::Local(expected, name, expression) => {
                    let value = self.expression(expression)?;
                    self.expect_type(*expected, value.ty, "local declaration")?;
                    self.env.insert(name.clone(), value);
                }
                Statement::Assign(target, expression) => {
                    let value = self.expression(expression)?;
                    self.assignment(target, value)?;
                }
                Statement::Require(expression) => {
                    let condition = self.expression(expression)?;
                    self.expect_type(ValueType::SecretBool, condition.ty, "require")?;
                    self.push_node(
                        "require_secret",
                        Representation::SecretSharing,
                        vec![condition.reference],
                        ValueType::Void,
                        None,
                    );
                }
                Statement::Return(expression) => {
                    let value = self.expression(expression)?;
                    if value.ty != ValueType::Opened {
                        return Err(self.error("only Pick-opened secret values may be returned"));
                    }
                    self.opening_output = Some(value.reference);
                }
                Statement::Expression(expression) => {
                    self.expression(expression)?;
                }
            }
        }

        let inputs = self
            .function
            .params
            .iter()
            .map(|param| AbiInput {
                name: param.name.clone(),
                source_type: param.ty,
                abi_type: param.ty.abi_type().to_owned(),
                representation: param.ty.representation(),
            })
            .collect::<Vec<_>>();
        let abi_types = inputs
            .iter()
            .map(|input| input.abi_type.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let signature = format!("{}({abi_types})", self.function.name);
        Ok(FunctionManifest {
            name: self.function.name.clone(),
            selector: selector(&signature),
            signature,
            inputs,
            operators: self.operators,
            state_writes: self.writes,
            opening_output: self.opening_output,
        })
    }

    fn assignment(&mut self, target: &Expression, value: TypedRef) -> Result<()> {
        match target {
            Expression::Index(state_name, key_expression) => {
                let state = self.state(state_name)?.clone();
                let key = self.expression(key_expression)?;
                let expected_key = state
                    .key_ty
                    .ok_or_else(|| self.error("scalar state cannot be indexed"))?;
                self.expect_type(expected_key, key.ty, "state key")?;
                self.expect_type(state.ty, value.ty, "state assignment")?;
                self.writes.push(StateWrite {
                    state: state_name.clone(),
                    key: Some(key.reference),
                    value: value.reference,
                });
                Ok(())
            }
            Expression::Name(state_name) => {
                let state = self.state(state_name)?;
                if state.key_ty.is_some() {
                    return Err(self.error("mapped state requires an index"));
                }
                self.expect_type(state.ty, value.ty, "state assignment")?;
                self.writes.push(StateWrite {
                    state: state_name.clone(),
                    key: None,
                    value: value.reference,
                });
                Ok(())
            }
            _ => Err(self.error("assignment target must be private state")),
        }
    }

    fn expression(&mut self, expression: &Expression) -> Result<TypedRef> {
        match expression {
            Expression::Name(name) => self
                .env
                .get(name)
                .cloned()
                .ok_or_else(|| self.error(&format!("unknown value `{name}`"))),
            Expression::Number(value) => Ok(TypedRef {
                ty: ValueType::Uint,
                reference: format!("const:{value}"),
            }),
            Expression::Index(name, key_expression) => {
                let state = self.state(name)?.clone();
                let key = self.expression(key_expression)?;
                let expected_key = state
                    .key_ty
                    .ok_or_else(|| self.error("scalar state cannot be indexed"))?;
                self.expect_type(expected_key, key.ty, "state key")?;
                let id = self.push_node(
                    "load_state",
                    state.ty.representation(),
                    vec![key.reference],
                    state.ty,
                    Some(name.clone()),
                );
                Ok(TypedRef {
                    ty: state.ty,
                    reference: id,
                })
            }
            Expression::Call(name, arguments) => self.call(name, arguments),
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression]) -> Result<TypedRef> {
        let mut args = Vec::with_capacity(arguments.len());
        for argument in arguments {
            args.push(self.expression(argument)?);
        }
        let (expected, output, opcode, domain) = match name {
            "FHE.encrypt" => (
                vec![ValueType::Uint],
                ValueType::FheUint,
                "fhe.encrypt",
                Representation::Fhe,
            ),
            "FHE.add" => (
                vec![ValueType::FheUint, ValueType::FheUint],
                ValueType::FheUint,
                "fhe.add",
                Representation::Fhe,
            ),
            "FHE.sub" => (
                vec![ValueType::FheUint, ValueType::FheUint],
                ValueType::FheUint,
                "fhe.sub",
                Representation::Fhe,
            ),
            "FHE.ge" => (
                vec![ValueType::FheUint, ValueType::FheUint],
                ValueType::FheBool,
                "fhe.ge",
                Representation::Fhe,
            ),
            "MPC.add" => (
                vec![ValueType::Sint, ValueType::Sint],
                ValueType::Sint,
                "mpc.add",
                Representation::SecretSharing,
            ),
            "MPC.sub" => (
                vec![ValueType::Sint, ValueType::Sint],
                ValueType::Sint,
                "mpc.sub",
                Representation::SecretSharing,
            ),
            "MPC.ge" => (
                vec![ValueType::Sint, ValueType::Sint],
                ValueType::SecretBool,
                "mpc.ge",
                Representation::SecretSharing,
            ),
            "H2S" => match args.first().map(|arg| arg.ty) {
                Some(ValueType::FheUint) => (
                    vec![ValueType::FheUint],
                    ValueType::Sint,
                    "convert.h2s",
                    Representation::SecretSharing,
                ),
                Some(ValueType::FheBool) => (
                    vec![ValueType::FheBool],
                    ValueType::SecretBool,
                    "convert.h2s",
                    Representation::SecretSharing,
                ),
                _ => return Err(self.error("H2S expects FheUint or FheBool")),
            },
            "S2H" => (
                vec![ValueType::Sint],
                ValueType::FheUint,
                "convert.s2h",
                Representation::Fhe,
            ),
            "Pick" => (
                vec![ValueType::Sint],
                ValueType::Opened,
                "opening.pick",
                Representation::Public,
            ),
            "receiveEncryptedToken" => (
                vec![ValueType::FheUint],
                ValueType::Void,
                "host.receive_encrypted_token",
                Representation::Fhe,
            ),
            "sendEncryptedToken" => (
                vec![ValueType::Address, ValueType::FheUint],
                ValueType::Void,
                "host.send_encrypted_token",
                Representation::Fhe,
            ),
            _ => return Err(self.error(&format!("unknown operation `{name}`"))),
        };
        if args.len() != expected.len() {
            return Err(self.error(&format!(
                "operation `{name}` expects {} arguments, got {}",
                expected.len(),
                args.len()
            )));
        }
        for (index, (actual, wanted)) in args.iter().zip(expected.iter()).enumerate() {
            if actual.ty != *wanted {
                return Err(self.error(&format!(
                    "argument {} of `{name}` expects {:?}, got {:?}; use H2S/S2H for representation changes",
                    index + 1,
                    wanted,
                    actual.ty
                )));
            }
        }
        let id = self.push_node(
            opcode,
            domain,
            args.into_iter().map(|arg| arg.reference).collect(),
            output,
            None,
        );
        Ok(TypedRef {
            ty: output,
            reference: id,
        })
    }

    fn state(&self, name: &str) -> Result<&StateDecl> {
        self.program
            .states
            .iter()
            .find(|state| state.name == name)
            .ok_or_else(|| self.error(&format!("unknown private state `{name}`")))
    }

    fn expect_type(&self, expected: ValueType, actual: ValueType, context: &str) -> Result<()> {
        if expected == actual {
            Ok(())
        } else {
            Err(self.error(&format!(
                "{context} expects {:?}, got {:?}",
                expected, actual
            )))
        }
    }

    fn push_node(
        &mut self,
        opcode: &str,
        domain: Representation,
        inputs: Vec<String>,
        output_type: ValueType,
        state: Option<String>,
    ) -> String {
        let id = format!("op{}", self.operators.len());
        self.operators.push(OperatorNode {
            id: id.clone(),
            opcode: opcode.to_owned(),
            domain,
            inputs,
            output_type,
            representation: output_type.representation(),
            state,
        });
        id
    }

    fn error(&self, message: &str) -> CompileError {
        CompileError {
            message: format!("function `{}`: {message}", self.function.name),
            offset: 0,
        }
    }
}

/// Parse, type-check and lower a privacy contract into a runtime manifest.
pub fn compile(source: &str) -> Result<ContractManifest> {
    let program = Parser::new(source)?.parse()?;
    let mut names = BTreeMap::new();
    let mut state = Vec::new();
    for declaration in &program.states {
        if !matches!(declaration.ty, ValueType::FheUint | ValueType::Sint) {
            return Err(CompileError {
                message: format!(
                    "private state `{}` must be FheUint or Sint",
                    declaration.name
                ),
                offset: 0,
            });
        }
        if declaration.key_ty.is_some() && declaration.key_ty != Some(ValueType::Address) {
            return Err(CompileError {
                message: format!(
                    "private mapping `{}` currently requires address keys",
                    declaration.name
                ),
                offset: 0,
            });
        }
        if names.insert(&declaration.name, ()).is_some() {
            return Err(CompileError {
                message: format!("duplicate state `{}`", declaration.name),
                offset: 0,
            });
        }
        state.push(StateManifest {
            name: declaration.name.clone(),
            value_type: declaration.ty,
            key_type: declaration.key_ty,
            representation: declaration.ty.representation(),
        });
    }
    let mut functions = Vec::new();
    for function in &program.functions {
        functions.push(Lowerer::new(&program, function).lower()?);
    }
    Ok(ContractManifest {
        language_version: 1,
        contract: program.name,
        state,
        functions,
    })
}

/// Compile a source file and write deterministic runtime artifacts.
pub fn compile_file(
    source_path: &Path,
    output_root: &Path,
) -> std::result::Result<PathBuf, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(source_path)?;
    let manifest = compile(&source)?;
    let output_dir = output_root.join(&manifest.contract);
    fs::create_dir_all(&output_dir)?;
    let pretty = serde_json::to_string_pretty(&manifest)?;
    fs::write(output_dir.join("manifest.json"), format!("{pretty}\n"))?;
    let operators = manifest
        .functions
        .iter()
        .map(|function| (&function.name, &function.operators))
        .collect::<BTreeMap<_, _>>();
    fs::write(
        output_dir.join("operators.json"),
        format!("{}\n", serde_json::to_string_pretty(&operators)?),
    )?;
    let abi = manifest
        .functions
        .iter()
        .map(|function| {
            serde_json::json!({
                "type": "function",
                "name": function.name,
                "selector": function.selector,
                "inputs": function.inputs.iter().map(|input| serde_json::json!({
                    "name": input.name,
                    "type": input.abi_type,
                    "internalType": format!("{:?}", input.source_type),
                })).collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        output_dir.join("abi.json"),
        format!("{}\n", serde_json::to_string_pretty(&abi)?),
    )?;
    let function_hashes = manifest
        .functions
        .iter()
        .map(|function| {
            format!(
                "{}_SELECTOR={}",
                function.name.to_ascii_uppercase(),
                function.selector
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let hashes = format!(
        "MANIFEST_HASH={}\nRUNTIME_HASH={}\n{function_hashes}",
        keccak_hex(pretty.as_bytes()),
        keccak_hex(b"PPSC_MANIFEST_RUNTIME_V1")
    );
    fs::write(output_dir.join("hashes.env"), format!("{hashes}\n"))?;
    Ok(output_dir)
}

pub fn manifest_from_path(
    path: &Path,
) -> std::result::Result<ContractManifest, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

/// Generate the user-facing Solidity gateway for a compiled manifest.
/// `control_import` is relative to the generated Solidity file.
pub fn generate_solidity_gateway(
    manifest: &ContractManifest,
    control_import: &str,
) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let contract_name = format!("{}Gateway", manifest.contract);
    let mut source = format!(
        "// SPDX-License-Identifier: Apache-2.0\npragma solidity ^0.8.24;\n\nimport {{ PpscControlPlane }} from \"{control_import}\";\n\n/// @notice Generated by ppsc-compiler. Do not edit by hand.\ncontract {contract_name} {{\n    error Unauthorized();\n\n    PpscControlPlane public immutable controlPlane;\n    bytes32 public immutable confidentialContractId;\n    mapping(bytes32 => bytes) public openingResults;\n\n"
    );
    for function in &manifest.functions {
        source.push_str(&format!(
            "    bytes4 public constant {}_SELECTOR = {};\n",
            solidity_constant(&function.name),
            function.selector
        ));
    }
    source.push_str(
        "\n    constructor(\n        PpscControlPlane controlPlane_,\n        bytes32 deploymentSalt,\n        bytes32 manifestHash,\n        bytes32 runtimeHash,\n        bytes32 initialStateRoot,\n        string memory codeLocation\n    ) {\n        controlPlane = controlPlane_;\n        confidentialContractId = controlPlane_.publishContract(\n            deploymentSalt, manifestHash, runtimeHash, initialStateRoot\n        );\n",
    );
    for function in &manifest.functions {
        let program_json = serde_json::to_vec(function)?;
        let operators_json = serde_json::to_vec(&function.operators)?;
        let abi_json = serde_json::to_vec(&function.inputs)?;
        source.push_str(&format!(
            "        controlPlane_.publishFunction(\n            confidentialContractId,\n            {}_SELECTOR,\n            {},\n            {},\n            {},\n            string.concat(codeLocation, \"#{}\"),\n            {}\n        );\n",
            solidity_constant(&function.name),
            keccak_hex(&program_json),
            keccak_hex(&operators_json),
            keccak_hex(&abi_json),
            function.name,
            function.operators.len().max(1) * 50_000
        ));
    }
    source.push_str("    }\n\n");

    for state in &manifest.state {
        let name_hash = keccak_hex(state.name.as_bytes());
        match state.key_type {
            Some(ValueType::Address) => source.push_str(&format!(
                "    function {}Variable(address key) public view returns (bytes32) {{\n        return keccak256(abi.encode(\n            \"PPSC_STATE_VARIABLE_V1\", confidentialContractId, {name_hash}, bytes32(uint256(uint160(key)))\n        ));\n    }}\n\n",
                state.name
            )),
            None => source.push_str(&format!(
                "    function {}Variable() public view returns (bytes32) {{\n        return keccak256(abi.encode(\n            \"PPSC_STATE_VARIABLE_V1\", confidentialContractId, {name_hash}, bytes32(0)\n        ));\n    }}\n\n",
                state.name
            )),
            _ => {}
        }
    }

    for function in &manifest.functions {
        source.push_str(&solidity_function(function));
    }
    source.push_str(
        "    /// @notice Development callback. Production runtimes encrypt Pick output to the requester.\n    function fulfillOpening(bytes32 executionId, bytes calldata result) external {\n        if (msg.sender != controlPlane.runtime()) revert Unauthorized();\n        if (controlPlane.executionStatus(executionId) != PpscControlPlane.ExecutionStatus.Completed) {\n            revert Unauthorized();\n        }\n        openingResults[executionId] = result;\n    }\n",
    );
    source.push_str("}\n");
    Ok(source)
}

pub fn write_solidity_gateway(
    manifest: &ContractManifest,
    output_dir: &Path,
) -> std::result::Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(output_dir)?;
    let path = output_dir.join(format!("{}Gateway.sol", manifest.contract));
    fs::write(
        &path,
        generate_solidity_gateway(manifest, "../PpscControlPlane.sol")?,
    )?;
    Ok(path)
}

fn solidity_function(function: &FunctionManifest) -> String {
    let mut parameters = Vec::new();
    let mut private_inputs = Vec::new();
    let mut public_inputs = Vec::new();
    for input in &function.inputs {
        match input.source_type {
            ValueType::Address => {
                parameters.push(format!("address {}", input.name));
                public_inputs.push(input.name.clone());
            }
            ValueType::Uint => {
                parameters.push(format!("uint256 {}", input.name));
                public_inputs.push(input.name.clone());
            }
            ValueType::FheUint | ValueType::Sint => {
                let parameter = format!("{}DataId", input.name);
                parameters.push(format!("bytes32 {parameter}"));
                private_inputs.push(parameter);
            }
            _ => {}
        }
    }
    parameters.push("uint64 nonce".to_owned());
    parameters.push("uint64 deadline".to_owned());
    let mut body = format!(
        "    function {}({}) external returns (bytes32 executionId) {{\n        bytes32[] memory privateInputs = new bytes32[]({});\n",
        function.name,
        parameters.join(", "),
        private_inputs.len()
    );
    for (index, parameter) in private_inputs.iter().enumerate() {
        body.push_str(&format!("        privateInputs[{index}] = {parameter};\n"));
    }
    let public_encoding = if public_inputs.is_empty() {
        "bytes(\"\")".to_owned()
    } else {
        format!("abi.encode({})", public_inputs.join(", "))
    };
    body.push_str(&format!(
        "        executionId = controlPlane.invokeCompiledFor(\n            confidentialContractId, {}_SELECTOR, privateInputs, {public_encoding}, msg.sender, nonce, deadline\n        );\n    }}\n\n",
        solidity_constant(&function.name)
    ));
    body
}

fn solidity_constant(name: &str) -> String {
    let mut output = String::new();
    for (index, character) in name.chars().enumerate() {
        if character.is_ascii_uppercase() && index != 0 {
            output.push('_');
        }
        output.push(character.to_ascii_uppercase());
    }
    output
}

fn keccak_hex(bytes: &[u8]) -> String {
    let mut digest = [0_u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    hasher.finalize(&mut digest);
    format!("0x{}", hex(&digest))
}

fn selector(signature: &str) -> String {
    let mut digest = [0_u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut digest);
    format!("0x{}", hex(&digest[..4]))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAPER_CONTRACT: &str = r#"
        privacy contract ConfidentialToken {
          private FheUint balance[address];
          function transfer(address to, FheUint amount) public {
            SecretBool sufficient = H2S(FHE.ge(balance[msg.sender], amount));
            require(sufficient);
            balance[msg.sender] = FHE.sub(balance[msg.sender], amount);
            balance[to] = FHE.add(balance[to], amount);
          }
          function getBalance() public {
            Sint balanceShare = H2S(balance[msg.sender]);
            return Pick(balanceShare);
          }
        }
    "#;

    #[test]
    fn compiles_paper_contract_to_operator_dag() {
        let manifest = compile(PAPER_CONTRACT).expect("paper contract should compile");
        let transfer = manifest
            .functions
            .iter()
            .find(|function| function.name == "transfer")
            .expect("transfer exists");
        assert_eq!(transfer.signature, "transfer(address,bytes32)");
        let opcodes = transfer
            .operators
            .iter()
            .map(|node| node.opcode.as_str())
            .collect::<Vec<_>>();
        assert!(opcodes.contains(&"fhe.ge"));
        assert!(opcodes.contains(&"convert.h2s"));
        assert!(opcodes.contains(&"require_secret"));
        let query = manifest
            .functions
            .iter()
            .find(|function| function.name == "getBalance")
            .expect("getBalance exists");
        assert!(query
            .operators
            .iter()
            .any(|node| node.opcode == "opening.pick"));
        assert!(query.opening_output.is_some());
    }

    #[test]
    fn output_is_deterministic() {
        let first =
            serde_json::to_string(&compile(PAPER_CONTRACT).expect("compile")).expect("json");
        let second =
            serde_json::to_string(&compile(PAPER_CONTRACT).expect("compile")).expect("json");
        assert_eq!(first, second);
    }

    #[test]
    fn rejects_implicit_cross_domain_operation() {
        let source = r#"
            privacy contract Bad {
              private FheUint balance[address];
              function add(FheUint amount) public {
                Sint value = MPC.add(balance[msg.sender], amount);
              }
            }
        "#;
        let error = compile(source).expect_err("implicit conversion must fail");
        assert!(error.message.contains("use H2S/S2H"));
    }

    #[test]
    fn require_needs_secret_boolean() {
        let source = r#"
            privacy contract Bad {
              private FheUint balance[address];
              function check() public { require(1); }
            }
        "#;
        let error = compile(source).expect_err("public integer require must fail");
        assert!(error.message.contains("expects SecretBool"));
    }

    #[test]
    fn generates_deployable_gateway_using_manifest_selectors() {
        let manifest = compile(PAPER_CONTRACT).expect("compile");
        let source = generate_solidity_gateway(&manifest, "../PpscControlPlane.sol")
            .expect("generate gateway");
        assert!(source.contains("contract ConfidentialTokenGateway"));
        assert!(source.contains("TRANSFER_SELECTOR = 0x7d32e7bd"));
        assert!(source.contains(
            "function transfer(address to, bytes32 amountDataId, uint64 nonce, uint64 deadline)"
        ));
        assert!(source.contains("controlPlane.invokeCompiledFor("));
        assert!(source.contains("function balanceVariable(address key)"));
    }
}
