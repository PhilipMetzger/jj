// Copyright 2025 The Jujutsu Authors
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

//! This file contains the no-op `jj script` command, which evaluates
//! userscripts and simplifies third-party integrations like `jj-fzf`.

use std::sync::Arc;

use clap::arg;
use jj_lib::repo::Repo;
use jj_lib::store::Store;
use starlark::any::ProvidesStaticType;
use starlark::environment::Globals;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::values::Heap;
use starlark::values::Value;

use crate::cli_util::CommandHelper;
use crate::cli_util::WorkspaceCommandHelper;
use crate::command_error::CommandError;
use crate::command_error::cli_error;
use crate::ui::Ui;

/// Execute a `.jjs` script
#[derive(Debug, Clone, clap::Args)]
pub struct ScriptArgs {
    /// Read all script input from stdin and only commit the transaction on EOD.
    #[arg(long)]
    repl: bool,
    // TODO: script: MahouScript
}

const BASE_SCRIPT: &'static str = r#"
# main must not return anything
def main(ctx: JujutsuContext):
   repo = ctx.repo
   last_commit = ctx.repo.tip
   print(f"JJ Script: {repo.name}")
   print(f"author: {last_commit.author()}")
   print(f"files changed: {[for f in last_commit.files().changed(): f.name]}")
   print(f"parents: {[for p in last_commit.parents(): p.c()]}")

# mahou.main(
#     impl = _main,
#     cli_args = {
#       "revision": cli_args.list(cli_arg.Revision(), doc = "The revisions to pass"),
#       "reset":  cli_args.bool(required = true)
#
# 
#     }
"#;

pub fn cmd_script(
    _ui: &mut Ui,
    _command: &CommandHelper,
    args: &ScriptArgs,
) -> Result<(), CommandError> {
    let mut interpreter = mahou::Interpreter::new();
    let mut helper = _command.workspace_helper(_ui)?;
    interpreter.run_script(BASE_SCRIPT)?;
    let module = Module::new();
    let ast = AstModule::parse("test.star", BASE_SCRIPT.to_string(), &Dialect::Extended)
        .map_err(|e| cli_error(e.into_anyhow()))?;
    let global_builder = GlobalsBuilder::new().with(jj_globals);
    let mut eval = Evaluator::new(&module);
    let context = JujutsuContext::new(_command.settings(), &helper);
    let tx = helper.start_transaction();
    eval.extra = Some(&context);
    // TODO: everything here
    if args.repl {
        interpreter.run_repl()?;
        return Ok(());
        // let mut repl = rustyline::Editor::new()?;
        // let line = repl.readline(">> ");
    }
    // TODO: Inject a JujutsuContext here, like bxl.
    // TODO: We also need to now the what the currently checked out revision is and
    // what the latest revision in the repo is to inject into the context (I
    // asked if something like it exists here: https://discord.com/channels/968932220549103686/969291218347524238/1358818101734543380)
    eval.eval_module(ast, &global_builder.build()).unwrap();
    // let result = interp.run(...)?;
    tx.finish(&_ui, "script: ...");
    Ok(())
}
