//! Functions for converting an AST to TIR.
use std::rc::Rc;
use std::fs::File;
use std::io::Read;
use std::path::MAIN_SEPARATOR;
use std::collections::HashMap;

use config::Config;
use default_globals::DEFAULT_GLOBALS;
use diagnostics::Diagnostics;
use mutability::Mutability;
use parser::{Parser, Node};
use symbol::RcSymbol;
use symbol_table::SymbolTable;
use tir::code_object::CodeObject;
use tir::expression::{Argument, Expression};
use tir::implement::{Implement, Rename};
use tir::import::Symbol as ImportSymbol;
use tir::module::Module;
use tir::raw_instructions::*;
use types::Type;
use types::array::Array;
use types::block::Block;
use types::database::Database as TypeDatabase;
use types::float::Float;
use types::integer::Integer;
use types::object::Object;
use types::string::String as StringType;

pub struct Builder {
    pub config: Rc<Config>,

    /// Any diagnostics that were produced when compiling modules.
    pub diagnostics: Diagnostics,

    /// All the compiled modules, mapped to their names. The values of this hash
    /// are explicitly set to None when:
    ///
    /// * The module was found and is about to be processed for the first time
    /// * The module could not be found
    ///
    /// This prevents recursive imports from causing the compiler to get stuck
    /// in a loop.
    pub modules: HashMap<String, Option<Module>>,

    /// The database storing all type information.
    pub typedb: TypeDatabase,
}

struct Context<'a> {
    /// The path of the module that is being compiled.
    path: &'a String,

    /// The local variables for the current scope.
    locals: &'a mut SymbolTable,

    /// The module locals for the currently compiled module.
    globals: &'a mut SymbolTable,

    /// The ID of the next temporary to set.
    temporary_id: usize,
}

impl<'a> Context<'a> {
    pub fn new(
        path: &'a String,
        locals: &'a mut SymbolTable,
        globals: &'a mut SymbolTable,
    ) -> Self {
        Context {
            path: path,
            locals: locals,
            globals: globals,
            temporary_id: 0,
        }
    }

    pub fn new_temporary(&mut self) -> usize {
        let id = self.temporary_id;

        self.temporary_id += 1;

        id
    }
}

impl Builder {
    pub fn new(config: Rc<Config>) -> Self {
        Builder {
            config: config,
            diagnostics: Diagnostics::new(),
            modules: HashMap::new(),
            typedb: TypeDatabase::new(),
        }
    }

    /// Builds the main module that starts the application.
    pub fn build_main(&mut self, path: String) -> Option<Module> {
        let name = self.module_name_for_path(&path);

        self.build(name, path)
    }

    pub fn build(&mut self, name: String, path: String) -> Option<Module> {
        let module = if let Ok(ast) = self.parse_file(&path) {
            let module = self.module(name, path, ast);

            Some(module)
        } else {
            None
        };

        module
    }

    fn module(&mut self, name: String, path: String, node: Node) -> Module {
        let mut globals = self.module_globals();
        let kind = Type::Object(Object::new());
        let locals = self.symbol_table_with_self(kind.clone());

        let code_object =
            self.code_object_with_locals(&path, &node, locals, &mut globals);

        let body = Expression::DefineModule {
            name: Box::new(self.string(name.clone(), 1, 1)),
            body: code_object,
            line: 1,
            column: 1,
            kind: kind,
        };

        Module {
            path: path,
            name: name,
            body: body,
            globals: globals,
        }
    }

    fn code_object(
        &mut self,
        path: &String,
        node: &Node,
        globals: &mut SymbolTable,
    ) -> CodeObject {
        self.code_object_with_locals(path, node, SymbolTable::new(), globals)
    }

    fn code_object_with_locals(
        &mut self,
        path: &String,
        node: &Node,
        mut locals: SymbolTable,
        globals: &mut SymbolTable,
    ) -> CodeObject {
        let body = match node {
            &Node::Expressions { ref nodes } => {
                let mut context = Context::new(path, &mut locals, globals);

                self.process_nodes(nodes, &mut context)
            }
            _ => Vec::new(),
        };

        CodeObject::new(locals, body)
    }

    fn process_nodes(
        &mut self,
        nodes: &Vec<Node>,
        context: &mut Context,
    ) -> Vec<Expression> {
        nodes
            .iter()
            .map(|ref node| self.process_node(node, context))
            .collect()
    }

    fn process_node(&mut self, node: &Node, context: &mut Context) -> Expression {
        match node {
            &Node::Integer { value, line, column } => {
                self.integer(value, line, column)
            }
            &Node::Float { value, line, column } => {
                self.float(value, line, column)
            }
            &Node::String { ref value, line, column } => {
                self.string(value.clone(), line, column)
            }
            &Node::Array { ref values, line, column } => {
                self.array_from_ast(values, line, column, context)
            }
            &Node::Hash { ref pairs, line, column } => {
                self.hash(pairs, line, column, context)
            }
            &Node::SelfObject { line, column } => {
                self.get_self(line, column, context)
            }
            &Node::Identifier { ref name, line, column } => {
                self.identifier(name, line, column, context)
            }
            &Node::Attribute { ref name, line, column } => {
                self.attribute(name.clone(), line, column, context)
            }
            &Node::Constant { ref receiver, ref name, line, column } => {
                self.get_constant(name.clone(), receiver, line, column, context)
            }
            &Node::Type { ref constant, .. } => {
                // TODO: actually use type info from Type nodes
                self.process_node(constant, context)
            }
            &Node::LetDefine { ref name, ref value, line, column, .. } => {
                self.set_variable(
                    name,
                    value,
                    Mutability::Immutable,
                    line,
                    column,
                    context,
                )
            }
            &Node::VarDefine { ref name, ref value, line, column, .. } => {
                self.set_variable(
                    name,
                    value,
                    Mutability::Mutable,
                    line,
                    column,
                    context,
                )
            }
            &Node::Send {
                ref name,
                ref receiver,
                ref arguments,
                line,
                column,
            } => {
                self.send_object_message(
                    name.clone(),
                    receiver,
                    arguments,
                    line,
                    column,
                    context,
                )
            }
            &Node::Import { ref steps, ref symbols, line, column } => {
                self.import(steps, symbols, line, column, context)
            }
            &Node::Closure { ref arguments, ref body, line, column, .. } => {
                self.closure(arguments, body, line, column, context)
            }
            &Node::KeywordArgument { ref name, ref value, line, column } => {
                self.keyword_argument(name.clone(), value, line, column, context)
            }
            &Node::Method {
                ref name,
                ref receiver,
                ref arguments,
                ref body,
                line,
                column,
                ..
            } => {
                if let &Some(ref body) = body {
                    self.method(
                        name.clone(),
                        receiver,
                        arguments,
                        body,
                        line,
                        column,
                        context,
                    )
                } else {
                    self.required_method(
                        name.clone(),
                        receiver,
                        arguments,
                        line,
                        column,
                        context,
                    )
                }
            }
            &Node::Object {
                ref name,
                ref implements,
                ref body,
                line,
                column,
                ..
            } => {
                self.def_object(
                    name.clone(),
                    implements,
                    body,
                    line,
                    column,
                    context,
                )
            }
            &Node::Trait { ref name, ref body, line, column, .. } => {
                self.def_trait(name.clone(), body, line, column, context)
            }
            &Node::Return { ref value, line, column } => {
                self.return_value(value, line, column, context)
            }
            &Node::TypeCast { ref value, .. } => self.type_cast(value, context),
            &Node::Try {
                ref body,
                ref else_body,
                ref else_argument,
                line,
                column,
                ..
            } => self.try(body, else_body, else_argument, line, column, context),
            &Node::Throw { ref value, line, column } => {
                self.throw(value, line, column, context)
            }
            &Node::Add { ref left, ref right, line, column } => {
                self.op_add(left, right, line, column, context)
            }
            &Node::And { ref left, ref right, line, column } => {
                self.op_and(left, right, line, column, context)
            }
            &Node::BitwiseAnd { ref left, ref right, line, column } => {
                self.op_bitwise_and(left, right, line, column, context)
            }
            &Node::BitwiseOr { ref left, ref right, line, column } => {
                self.op_bitwise_or(left, right, line, column, context)
            }
            &Node::BitwiseXor { ref left, ref right, line, column } => {
                self.op_bitwise_xor(left, right, line, column, context)
            }
            &Node::Div { ref left, ref right, line, column } => {
                self.op_div(left, right, line, column, context)
            }
            &Node::Equal { ref left, ref right, line, column } => {
                self.op_equal(left, right, line, column, context)
            }
            &Node::Greater { ref left, ref right, line, column } => {
                self.op_greater(left, right, line, column, context)
            }
            &Node::GreaterEqual { ref left, ref right, line, column } => {
                self.op_greater_equal(left, right, line, column, context)
            }
            &Node::Lower { ref left, ref right, line, column } => {
                self.op_lower(left, right, line, column, context)
            }
            &Node::LowerEqual { ref left, ref right, line, column } => {
                self.op_lower_equal(left, right, line, column, context)
            }
            &Node::Mod { ref left, ref right, line, column } => {
                self.op_mod(left, right, line, column, context)
            }
            &Node::Mul { ref left, ref right, line, column } => {
                self.op_mul(left, right, line, column, context)
            }
            &Node::NotEqual { ref left, ref right, line, column } => {
                self.op_not_equal(left, right, line, column, context)
            }
            &Node::Or { ref left, ref right, line, column } => {
                self.op_or(left, right, line, column, context)
            }
            &Node::Pow { ref left, ref right, line, column } => {
                self.op_pow(left, right, line, column, context)
            }
            &Node::ShiftLeft { ref left, ref right, line, column } => {
                self.op_shift_left(left, right, line, column, context)
            }
            &Node::ShiftRight { ref left, ref right, line, column } => {
                self.op_shift_right(left, right, line, column, context)
            }
            &Node::Sub { ref left, ref right, line, column } => {
                self.op_sub(left, right, line, column, context)
            }
            &Node::InclusiveRange { ref left, ref right, line, column } => {
                self.op_inclusive_range(left, right, line, column, context)
            }
            &Node::ExclusiveRange { ref left, ref right, line, column } => {
                self.op_exclusive_range(left, right, line, column, context)
            }
            &Node::Reassign { ref variable, ref value, line, column } => {
                self.reassign(variable, value, line, column, context)
            }
            _ => Expression::Void,
        }
    }

    fn integer(&self, val: i64, line: usize, col: usize) -> Expression {
        let kind = Integer::new(self.typedb.integer_prototype.clone());

        Expression::Integer {
            value: val,
            line: line,
            column: col,
            kind: Type::Integer(kind),
        }
    }

    fn float(&self, val: f64, line: usize, col: usize) -> Expression {
        let kind = Float::new(self.typedb.float_prototype.clone());

        Expression::Float {
            value: val,
            line: line,
            column: col,
            kind: Type::Float(kind),
        }
    }

    fn string(&self, val: String, line: usize, col: usize) -> Expression {
        let kind = StringType::new(self.typedb.string_prototype.clone());

        Expression::String {
            value: val,
            line: line,
            column: col,
            kind: Type::String(kind),
        }
    }

    fn array_from_ast(
        &mut self,
        value_nodes: &Vec<Node>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let values = self.process_nodes(&value_nodes, context);

        self.array(values, line, col)
    }

    fn array(
        &mut self,
        values: Vec<Expression>,
        line: usize,
        col: usize,
    ) -> Expression {
        let kind = Array::new(self.typedb.array_prototype.clone());

        Expression::Array {
            values: values,
            line: line,
            column: col,
            kind: Type::Array(kind),
        }
    }

    fn hash(
        &mut self,
        pair_nodes: &Vec<(Node, Node)>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let pairs = pair_nodes
            .iter()
            .map(|&(ref k, ref v)| {
                (self.process_node(k, context), self.process_node(v, context))
            })
            .collect();

        Expression::Hash { pairs: pairs, line: line, column: col }
    }

    fn get_self(
        &mut self,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let local = context.locals.lookup(&self.config.self_variable()).expect(
            "self is not defined in this context",
        );

        self.get_local(local, line, col)
    }

    fn identifier(
        &mut self,
        name: &String,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        // TODO: look up methods before looking up globals
        if let Some(local) = context.locals.lookup(name) {
            return self.get_local(local, line, col);
        }

        if let Some(global) = context.globals.lookup(name) {
            return self.get_global(global, line, col);
        }

        // TODO: check if method exists for identifiers without receivers
        let args = Vec::new();

        self.send_object_message(name.clone(), &None, &args, line, col, context)
    }

    fn attribute(
        &mut self,
        name: String,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let receiver = self.get_self(line, col, context);

        Expression::GetAttribute {
            receiver: Box::new(receiver),
            name: Box::new(self.string(name, line, col)),
            line: line,
            column: col,
        }
    }

    fn get_local(
        &mut self,
        variable: RcSymbol,
        line: usize,
        col: usize,
    ) -> Expression {
        let kind = variable.kind.clone();

        Expression::GetLocal {
            variable: variable,
            line: line,
            column: col,
            kind: kind,
        }
    }

    fn get_global(
        &mut self,
        variable: RcSymbol,
        line: usize,
        col: usize,
    ) -> Expression {
        let kind = variable.kind.clone();

        Expression::GetGlobal {
            variable: variable,
            line: line,
            column: col,
            kind: kind,
        }
    }

    fn get_constant(
        &mut self,
        name: String,
        receiver: &Option<Box<Node>>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let rec_expr = if let &Some(ref node) = receiver {
            self.process_node(node, context)
        } else {
            self.get_self(line, col, context)
        };

        Expression::GetAttribute {
            receiver: Box::new(rec_expr),
            name: Box::new(self.string(name, line, col)),
            line: line,
            column: col,
        }
    }

    fn set_constant(
        &mut self,
        name: String,
        value: Expression,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.set_attribute(name, value, line, col, context)
    }

    fn set_variable(
        &mut self,
        name_node: &Node,
        value_node: &Node,
        mutability: Mutability,
        line: usize,
        column: usize,
        context: &mut Context,
    ) -> Expression {
        let value_expr = self.process_node(value_node, context);

        match name_node {
            &Node::Identifier { ref name, .. } => {
                self.set_local(
                    name.clone(),
                    value_expr,
                    mutability,
                    line,
                    column,
                    context,
                )
            }
            &Node::Constant { ref name, .. } => {
                if mutability == Mutability::Mutable {
                    self.diagnostics.mutable_constant_error(
                        context.path,
                        line,
                        column,
                    );
                }

                self.set_constant(name.clone(), value_expr, line, column, context)
            }
            &Node::Attribute { ref name, .. } => {
                self.set_attribute(
                    name.clone(),
                    value_expr,
                    line,
                    column,
                    context,
                )
            }
            _ => unreachable!(),
        }
    }

    fn set_local(
        &mut self,
        name: String,
        value: Expression,
        mutability: Mutability,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let kind = value.kind();

        Expression::SetLocal {
            variable: context.locals.define(name, kind.clone(), mutability),
            value: Box::new(value),
            line: line,
            column: col,
            kind: kind,
        }
    }

    fn set_attribute(
        &mut self,
        name: String,
        value: Expression,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let kind = value.kind().clone();

        // TODO: track mutability of attributes per receiver type
        Expression::SetAttribute {
            receiver: Box::new(self.get_self(line, col, context)),
            name: Box::new(self.string(name, line, col)),
            value: Box::new(value),
            line: line,
            column: col,
            kind: kind,
        }
    }

    fn send_object_message(
        &mut self,
        mut name: String,
        receiver_node: &Option<Box<Node>>,
        arguments: &Vec<Node>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let receiver = if let &Some(ref rec) = receiver_node {
            let raw_ins = match **rec {
                Node::Constant { ref name, .. } => {
                    name == self.config.raw_instruction_receiver()
                }
                _ => false,
            };

            if raw_ins {
                return self.raw_instruction(name, arguments, line, col, context);
            }

            self.process_node(rec, context)
        } else {
            if let Some(local) = context.locals.lookup(&name) {
                name = self.config.call_message();

                self.get_local(local, line, col)
            } else {
                self.get_self(line, col, context)
            }
        };

        let args = arguments
            .iter()
            .map(|arg| self.process_node(arg, context))
            .collect();

        Expression::SendObjectMessage {
            receiver: Box::new(receiver),
            name: Box::new(self.string(name, line, col)),
            arguments: args,
            line: line,
            column: col,
        }
    }

    fn raw_instruction(
        &mut self,
        name: String,
        arg_nodes: &Vec<Node>, // TODO: use
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        match name.as_ref() {
            GET_BLOCK_PROTOTYPE => self.get_block_prototype(line, col),
            GET_INTEGER_PROTOTYPE => self.get_integer_prototype(line, col),
            GET_FLOAT_PROTOTYPE => self.get_float_prototype(line, col),
            GET_STRING_PROTOTYPE => self.get_string_prototype(line, col),
            GET_ARRAY_PROTOTYPE => self.get_array_prototype(line, col),
            GET_BOOLEAN_PROTOTYPE => self.get_boolean_prototype(line, col),
            SET_OBJECT => self.set_object(arg_nodes, line, col, context),
            GET_TOPLEVEL => self.get_toplevel(line, col),
            SET_ATTRIBUTE => {
                self.set_raw_attribute(arg_nodes, line, col, context)
            }
            _ => {
                self.diagnostics.unknown_raw_instruction_error(
                    &name,
                    context.path,
                    line,
                    col,
                );

                Expression::Void
            }
        }
    }

    fn get_block_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.block_prototype.clone());

        Expression::GetBlockPrototype { line: line, column: col, kind: kind }
    }

    fn get_integer_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.integer_prototype.clone());

        Expression::GetIntegerPrototype { line: line, column: col, kind: kind }
    }

    fn get_float_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.float_prototype.clone());

        Expression::GetFloatPrototype { line: line, column: col, kind: kind }
    }

    fn get_string_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.string_prototype.clone());

        Expression::GetStringPrototype { line: line, column: col, kind: kind }
    }

    fn get_array_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.array_prototype.clone());

        Expression::GetArrayPrototype { line: line, column: col, kind: kind }
    }

    fn get_boolean_prototype(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.boolean_prototype.clone());

        Expression::GetBooleanPrototype { line: line, column: col, kind: kind }
    }

    fn get_toplevel(&mut self, line: usize, col: usize) -> Expression {
        let kind = Type::Object(self.typedb.top_level.clone());

        Expression::GetTopLevel { line: line, column: col, kind: kind }
    }

    fn set_object(
        &mut self,
        arg_nodes: &Vec<Node>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let args = self.process_nodes(arg_nodes, context);

        Expression::SetObject {
            arguments: args,
            line: line,
            column: col,
            kind: Type::Object(Object::new()),
        }
    }

    fn set_raw_attribute(
        &mut self,
        arg_nodes: &Vec<Node>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        if arg_nodes.len() != 3 {
            panic!(
                "set_attribute requires 3 arguments, but {} were given",
                arg_nodes.len()
            );
        }

        let receiver = self.process_node(&arg_nodes[0], context);
        let attribute = self.process_node(&arg_nodes[1], context);
        let value = self.process_node(&arg_nodes[2], context);
        let kind = value.kind();

        Expression::SetAttribute {
            receiver: Box::new(receiver),
            name: Box::new(attribute),
            value: Box::new(value),
            line: line,
            column: col,
            kind: kind,
        }
    }

    /// Converts the list of import steps to a module name.
    fn module_steps_for_import(&self, steps: &Vec<Node>) -> Vec<String> {
        let mut chunks = Vec::new();

        for step in steps.iter() {
            match step {
                &Node::Identifier { ref name, .. } => {
                    chunks.push(name.clone());
                }
                &Node::Constant { .. } => break,
                _ => {}
            }
        }

        chunks
    }

    /// Returns a vector of symbols to import, based on a list of AST nodes
    /// describing the import steps.
    fn import_symbols(&self, symbol_nodes: &Vec<Node>) -> Vec<ImportSymbol> {
        let mut symbols = Vec::new();

        for node in symbol_nodes.iter() {
            match node {
                &Node::ImportSymbol {
                    symbol: ref symbol_node,
                    alias: ref alias_node,
                } => {
                    let alias = if let &Some(ref node) = alias_node {
                        self.name_of_node(node)
                    } else {
                        None
                    };

                    let symbol = match **symbol_node {
                        Node::Identifier { ref name, line, column } |
                        Node::Constant { ref name, line, column, .. } => {
                            let var_name = if let Some(alias) = alias {
                                alias
                            } else {
                                name.clone()
                            };

                            ImportSymbol::new(
                                name.clone(),
                                var_name,
                                line,
                                column,
                            )
                        }
                        _ => unreachable!(),
                    };

                    symbols.push(symbol);
                }
                _ => {}
            }
        }

        symbols
    }

    fn import(
        &mut self,
        step_nodes: &Vec<Node>,
        symbol_nodes: &Vec<Node>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let mod_steps = self.module_steps_for_import(step_nodes);
        let mod_path = self.module_path(&mod_steps);

        // We insert the module name before processing it to prevent the
        // compiler from getting stuck in a recursive import.
        if self.modules.get(&mod_path).is_none() {
            self.modules.insert(mod_path.clone(), None);

            match self.find_module_path(&mod_path) {
                Some(full_path) => {
                    let module = self.build(mod_path.clone(), full_path);

                    self.modules.insert(mod_path.clone(), module);
                }
                None => {
                    self.diagnostics.module_not_found_error(
                        &mod_path,
                        context.path,
                        line,
                        col,
                    );
                }
            };
        }

        let mut symbols = self.import_symbols(symbol_nodes);

        let step_strings = mod_steps
            .iter()
            .map(|string| self.string(string.clone(), line, col))
            .collect();

        let temp = context.new_temporary();

        // Example: get_toplevel.load_module(['std', 'string'])
        let load_module = Expression::SendObjectMessage {
            receiver: Box::new(self.get_toplevel(line, col)),
            name: Box::new(
                self.string(self.config.load_module_message(), line, col),
            ),
            arguments: vec![self.array(step_strings, line, col)],
            line: line,
            column: col,
        };

        let set_temp = Expression::SetTemporary {
            id: temp,
            value: Box::new(load_module),
            line: line,
            column: col,
        };

        let mut expressions = vec![set_temp];

        if symbols.is_empty() {
            // If no symbols are given the module itself is to be imported under
            // the same name.
            let mod_name = mod_steps.last().unwrap();
            let global = context.globals.define(
                mod_name.clone(),
                Type::Dynamic,
                Mutability::Immutable,
            );

            let kind = global.kind.clone();

            expressions.push(Expression::SetGlobal {
                variable: global,
                value: Box::new(Expression::GetTemporary {
                    id: temp,
                    line: line,
                    column: col,
                }),
                line: line,
                column: col,
                kind: kind,
            })
        } else {
            // If symbols _are_ given we will import the symbols into global
            // variables.
            for symbol in symbols.drain(0..) {
                let global = context.globals.define(
                    symbol.import_as,
                    Type::Dynamic,
                    Mutability::Immutable,
                );

                let global_kind = global.kind.clone();

                let value = Expression::SendObjectMessage {
                    receiver: Box::new(Expression::GetTemporary {
                        id: temp,
                        line: line,
                        column: col,
                    }),
                    name: Box::new(
                        self.string(self.config.symbol_message(), line, col),
                    ),
                    arguments: vec![self.string(symbol.import_name, line, col)],
                    line: symbol.line,
                    column: symbol.column,
                };

                expressions.push(Expression::SetGlobal {
                    variable: global,
                    value: Box::new(value),
                    line: symbol.line,
                    column: symbol.column,
                    kind: global_kind,
                });
            }
        }

        println!("{:#?}", expressions);

        Expression::Expressions { nodes: expressions }
    }

    fn closure(
        &mut self,
        arg_nodes: &Vec<Node>,
        body_node: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let arg_exprs = self.method_arguments(arg_nodes, context);
        let body = self.code_object(&context.path, body_node, context.globals);

        self.block(arg_exprs, body, line, col)
    }

    fn block(
        &self,
        arguments: Vec<Argument>,
        body: CodeObject,
        line: usize,
        col: usize,
    ) -> Expression {
        let kind = Block::new(self.typedb.block_prototype.clone());

        Expression::Block {
            arguments: arguments,
            body: body,
            line: line,
            column: col,
            kind: Type::Block(kind),
        }
    }

    fn keyword_argument(
        &mut self,
        name: String,
        value: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        Expression::KeywordArgument {
            name: name,
            value: Box::new(self.process_node(value, context)),
            line: line,
            column: col,
        }
    }

    fn method(
        &mut self,
        name: String,
        receiver: &Option<Box<Node>>,
        arg_nodes: &Vec<Node>,
        body: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let method_name = self.string(name, line, col);
        let arguments = self.method_arguments(arg_nodes, context);
        let mut locals = self.symbol_table_with_self(Type::Dynamic);

        for arg in arguments.iter() {
            locals.define(arg.name.clone(), Type::Dynamic, Mutability::Immutable);
        }

        let receiver_expr = if let &Some(ref r) = receiver {
            self.process_node(r, context)
        } else {
            self.get_self(line, col, context)
        };

        let body_expr = self.code_object_with_locals(
            &context.path,
            body,
            locals,
            context.globals,
        );

        let block = self.block(arguments, body_expr, line, col);
        let vkind = block.kind();

        Expression::SetAttribute {
            receiver: Box::new(receiver_expr),
            name: Box::new(method_name),
            value: Box::new(block),
            line: line,
            column: col,
            kind: vkind,
        }
    }

    fn required_method(
        &mut self,
        name: String,
        receiver: &Option<Box<Node>>,
        _arguments: &Vec<Node>, // TODO: use
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let receiver = if let &Some(ref rec) = receiver {
            self.process_node(rec, context)
        } else {
            self.get_self(line, col, context)
        };

        let method_name = self.string(name, line, col);
        let message_name =
            self.string(self.config.define_required_method_message(), line, col);

        Expression::SendObjectMessage {
            receiver: Box::new(receiver),
            name: Box::new(message_name),
            arguments: vec![method_name],
            line: line,
            column: col,
        }
    }

    fn method_arguments(
        &mut self,
        nodes: &Vec<Node>,
        context: &mut Context,
    ) -> Vec<Argument> {
        nodes
            .iter()
            .map(|node| match node {
                &Node::ArgumentDefine {
                    ref name,
                    ref default,
                    line,
                    column,
                    rest,
                    ..
                } => {
                    let default_val = default.as_ref().map(|node| {
                        self.process_node(node, context)
                    });

                    Argument {
                        name: name.clone(),
                        default_value: default_val,
                        line: line,
                        column: column,
                        rest: rest,
                    }
                }
                _ => unreachable!(),
            })
            .collect()
    }

    /// Generates the TIR for object definitions
    ///
    /// Object definitions are compiled down into simple message sends,
    /// attribute assignments, and the execution of a block. Take for example
    /// the following code:
    ///
    ///     object Person {
    ///       fn init(name) {
    ///         let @name = name
    ///       }
    ///     }
    ///
    /// This is compiled (roughly) into the following:
    ///
    ///     let Person = Object.new
    ///
    ///     fn(self) {
    ///       fn self.init(name) {
    ///         let @name = name
    ///       }
    ///
    ///       ...
    ///     }.call(Person)
    fn def_object(
        &mut self,
        name: String,
        _implements: &Vec<Node>, // TODO: use
        body: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let locals = self.symbol_table_with_self(Type::Dynamic);
        let global = self.lookup_object_constant(&context.globals);

        let object_new = Expression::SendObjectMessage {
            receiver: Box::new(self.get_global(global, line, col)),
            name: Box::new(self.string(self.config.new_message(), line, col)),
            arguments: Vec::new(),
            line: line,
            column: col,
        };

        let set_attr =
            self.set_attribute(name.clone(), object_new, line, col, context);

        let code_obj = self.code_object_with_locals(
            &context.path,
            body,
            locals,
            context.globals,
        );

        let block =
            self.block(vec![self.self_argument(line, col)], code_obj, line, col);

        let block_arg = self.attribute(name, line, col, context);

        let run_block = Expression::SendObjectMessage {
            receiver: Box::new(block),
            name: Box::new(self.string(self.config.call_message(), line, col)),
            arguments: vec![block_arg],
            line: line,
            column: col,
        };

        Expression::Expressions { nodes: vec![set_attr, run_block] }
    }

    fn def_trait(
        &mut self,
        name: String,
        body: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let locals = self.symbol_table_with_self(Type::Dynamic);
        let global = self.lookup_trait_constant(&context.globals);

        let object_new = Expression::SendObjectMessage {
            receiver: Box::new(self.get_global(global, line, col)),
            name: Box::new(self.string(self.config.new_message(), line, col)),
            arguments: Vec::new(),
            line: line,
            column: col,
        };

        let set_attr =
            self.set_attribute(name.clone(), object_new, line, col, context);

        let code_obj = self.code_object_with_locals(
            &context.path,
            body,
            locals,
            context.globals,
        );

        let block =
            self.block(vec![self.self_argument(line, col)], code_obj, line, col);

        let block_arg = self.attribute(name, line, col, context);

        let run_block = Expression::SendObjectMessage {
            receiver: Box::new(block),
            name: Box::new(self.string(self.config.call_message(), line, col)),
            arguments: vec![block_arg],
            line: line,
            column: col,
        };

        Expression::Expressions { nodes: vec![set_attr, run_block] }
    }

    fn implements(
        &mut self,
        nodes: &Vec<Node>,
        context: &mut Context,
    ) -> Vec<Implement> {
        nodes
            .iter()
            .map(|node| match node {
                &Node::Implement {
                    ref name, ref renames, line, column, ..
                } => self.implement(name, renames, line, column, context),
                _ => unreachable!(),
            })
            .collect()
    }

    fn implement(
        &mut self,
        name: &Node,
        rename_nodes: &Vec<(Node, Node)>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Implement {
        let renames = rename_nodes
            .iter()
            .map(|&(ref src, ref alias)| {
                let src_name = self.name_of_node(src).unwrap();
                let alias_name = self.name_of_node(alias).unwrap();

                Rename::new(src_name, alias_name)
            })
            .collect();

        Implement::new(self.process_node(name, context), renames, line, col)
    }

    fn return_value(
        &mut self,
        value: &Option<Box<Node>>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let ret_val = if let &Some(ref node) = value {
            Some(Box::new(self.process_node(node, context)))
        } else {
            None
        };

        Expression::Return { value: ret_val, line: line, column: col }
    }

    fn type_cast(&mut self, value: &Node, context: &mut Context) -> Expression {
        self.process_node(value, context)
    }

    fn try(
        &mut self,
        body: &Node,
        else_body: &Option<Box<Node>>,
        else_arg: &Option<Box<Node>>,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let body = self.code_object(&context.path, body, context.globals);

        let (else_body, else_arg) = if let &Some(ref node) = else_body {
            let mut else_locals = SymbolTable::new();

            let else_arg = if let &Some(ref node) = else_arg {
                let name = self.name_of_node(node).unwrap();

                Some(else_locals.define(
                    name,
                    Type::Dynamic,
                    Mutability::Immutable,
                ))
            } else {
                None
            };

            let body = self.code_object_with_locals(
                &context.path,
                node,
                else_locals,
                context.globals,
            );

            (Some(body), else_arg)
        } else {
            (None, None)
        };

        Expression::Try {
            body: body,
            else_body: else_body,
            else_argument: else_arg,
            line: line,
            column: col,
        }
    }

    fn throw(
        &mut self,
        value_node: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let value = self.process_node(value_node, context);

        Expression::Throw {
            value: Box::new(value),
            line: line,
            column: col,
        }
    }

    fn op_add(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "+", right, line, col, context)
    }

    fn op_and(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "&&", right, line, col, context)
    }

    fn op_bitwise_and(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "&", right, line, col, context)
    }

    fn op_bitwise_or(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "|", right, line, col, context)
    }

    fn op_bitwise_xor(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "^", right, line, col, context)
    }

    fn op_div(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "/", right, line, col, context)
    }

    fn op_equal(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "==", right, line, col, context)
    }

    fn op_greater(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, ">", right, line, col, context)
    }

    fn op_greater_equal(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, ">=", right, line, col, context)
    }

    fn op_lower(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "<", right, line, col, context)
    }

    fn op_lower_equal(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "<=", right, line, col, context)
    }

    fn op_mod(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "%", right, line, col, context)
    }

    fn op_mul(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "*", right, line, col, context)
    }

    fn op_not_equal(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "!=", right, line, col, context)
    }

    fn op_or(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "||", right, line, col, context)
    }

    fn op_pow(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "**", right, line, col, context)
    }

    fn op_shift_left(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "<<", right, line, col, context)
    }

    fn op_shift_right(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, ">>", right, line, col, context)
    }

    fn op_sub(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "-", right, line, col, context)
    }

    fn op_inclusive_range(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "..", right, line, col, context)
    }

    fn op_exclusive_range(
        &mut self,
        left: &Node,
        right: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        self.send_binary(left, "...", right, line, col, context)
    }

    fn reassign(
        &mut self,
        var_node: &Node,
        val_node: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let value = self.process_node(val_node, context);

        match var_node {
            &Node::Identifier { ref name, .. } => {
                if let Some(var) = context.locals.lookup(name) {
                    if !var.is_mutable() {
                        self.diagnostics.reassign_immutable_local_error(
                            name,
                            context.path,
                            line,
                            col,
                        );
                    }
                } else {
                    self.diagnostics.reassign_undefined_local_error(
                        name,
                        context.path,
                        line,
                        col,
                    );
                }

                self.set_local(
                    name.clone(),
                    value,
                    Mutability::Mutable,
                    line,
                    col,
                    context,
                )
            }
            &Node::Attribute { ref name, .. } => {
                // TODO: check for attribute existence
                self.set_attribute(name.clone(), value, line, col, context)
            }
            _ => unreachable!(),
        }
    }

    fn send_binary(
        &mut self,
        left_node: &Node,
        message: &str,
        right_node: &Node,
        line: usize,
        col: usize,
        context: &mut Context,
    ) -> Expression {
        let left = Box::new(self.process_node(left_node, context));
        let right = self.process_node(right_node, context);

        Expression::SendObjectMessage {
            receiver: left,
            name: Box::new(self.string(message.to_string(), line, col)),
            arguments: vec![right],
            line: line,
            column: col,
        }
    }

    fn name_of_node(&self, node: &Node) -> Option<String> {
        match node {
            &Node::Identifier { ref name, .. } |
            &Node::Constant { ref name, .. } => Some(name.clone()),
            _ => None,
        }
    }

    fn parse_file(&mut self, path: &String) -> Result<Node, ()> {
        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(err) => {
                self.diagnostics.error(path, err.to_string(), 1, 1);
                return Err(());
            }
        };

        let mut input = String::new();

        if let Err(err) = file.read_to_string(&mut input) {
            self.diagnostics.error(path, err.to_string(), 1, 1);
            return Err(());
        }

        let mut parser = Parser::new(&input);

        match parser.parse() {
            Ok(ast) => Ok(ast),
            Err(err) => {
                self.diagnostics.error(
                    path,
                    err,
                    parser.line(),
                    parser.column(),
                );

                Err(())
            }
        }
    }

    fn module_path(&self, steps: &Vec<String>) -> String {
        steps.join(&MAIN_SEPARATOR.to_string()) + self.config.source_extension()
    }


    fn module_name_for_path(&self, path: &String) -> String {
        if let Some(file_with_ext) = path.split(MAIN_SEPARATOR).last() {
            if let Some(file_name) = file_with_ext.split(".").next() {
                return file_name.to_string();
            }
        }

        "main".to_string()
    }

    fn find_module_path(&self, path: &str) -> Option<String> {
        for dir in self.config.source_directories.iter() {
            let full_path = dir.join(path);

            if full_path.exists() {
                return Some(full_path.to_str().unwrap().to_string());
            }
        }

        None
    }

    fn symbol_table_with_self(&self, kind: Type) -> SymbolTable {
        let mut table = SymbolTable::new();

        table.define(self.config.self_variable(), kind, Mutability::Immutable);

        table
    }

    fn self_argument(&self, line: usize, col: usize) -> Argument {
        Argument {
            name: self.config.self_variable(),
            default_value: None,
            line: line,
            column: col,
            rest: false,
        }
    }

    fn module_globals(&self) -> SymbolTable {
        let mut globals = SymbolTable::new();

        for &(_, global) in DEFAULT_GLOBALS.iter() {
            globals.define(
                global.to_string(),
                Type::Dynamic,
                Mutability::Immutable,
            );
        }

        globals
    }

    fn lookup_object_constant(&self, symbols: &SymbolTable) -> RcSymbol {
        symbols.lookup(self.config.object_constant()).unwrap()
    }

    fn lookup_trait_constant(&self, symbols: &SymbolTable) -> RcSymbol {
        symbols.lookup(self.config.trait_constant()).unwrap()
    }
}
