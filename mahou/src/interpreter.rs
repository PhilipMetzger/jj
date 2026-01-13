// Copyright 2026 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Entrypoint into the Mahou scripting language.

use starlark::environment::Globals;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::syntax::DialectTypes;

/// Common Errors from Mahou. 
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to parse file {} with {:?}", .0, .1)]
    ParseError(String, starlark::Error),
    #[error("failed to evaluate file")]
    EvalError(starlark::Error),
}

/// Contains all the state for Mahou, all registered globals and more.
#[derive(Debug)]
pub struct Interpreter {
    /// The global types and methods available.
    globals: Globals,
    /// We an extended dialect by default.
    dialect: Dialect,
}


impl Default for Interpreter {
    fn default() -> Self {
        let globals = Globals::extended_internal();
        Self {
            globals,
            dialect: Self::BUILTIN_DIALECT
        }
    }
}

const BUILTIN_FILE: &str = "toplevel";

impl Interpreter {
    const BUILTIN_DIALECT: Dialect = Dialect { enable_def: true, enable_lambda: true, enable_load: false, enable_keyword_only_arguments: true, enable_positional_only_arguments: true, enable_types: DialectTypes::Enable, enable_f_strings: true, ..Dialect::Standard };
    pub fn new() -> Self {
        Self::default()
    }
    
 

    pub fn eval_string(&mut self, str: String) -> Result<(), Error> {
        let ast = AstModule::parse(BUILTIN_FILE, str, &self.dialect)
            .map_err(|e| Error::ParseError(BUILTIN_FILE.to_string(), e))?;
        let module = Module::new();
        let mut eval = Evaluator::new(&module);
        let _value = eval.eval_module(ast, &self.globals).map_err(|e| Error::EvalError(e))?;
        Ok(())
    }
    
    
    fn find_entry_point<'v>(module: &'v Module) -> starlark::Result<()> {
        Ok(())
    }
}
