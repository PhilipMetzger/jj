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
use crate::command_error::cli_error;
use crate::command_error::CommandError;
use crate::ui::Ui;

/// Execute a `.jjs` script
#[derive(Debug, Clone, clap::Args)]
pub struct ScriptArgs {
    /// Read all script input from stdin and only commit the transaction on EOD.
    #[arg(long)]
    repl: bool,
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

# mahou_main(
#     impl = _main,
#     cli_args = {
#       "revision": cli_args.revision(required = true, doc = "some thing"),
#       "reset":  cli_args.bool(required = true)
#     })
#
"#;

pub fn cmd_script(
    _ui: &mut Ui,
    _command: &CommandHelper,
    _args: &ScriptArgs,
) -> Result<(), CommandError> {
    let helper = _command.workspace_helper(_ui)?;
    let module = Module::new();
    let ast = AstModule::parse("test.star", BASE_SCRIPT.to_string(), &Dialect::Extended)
        .map_err(|e| cli_error(e.into_anyhow()))?;
    let global_builder = GlobalsBuilder::new().with(jj_globals);
    let mut eval = Evaluator::new(&module);
    let context = JujutsuContext::new();
    eval.extra = Some(&context);
    // TODO: Inject a JujutsuContext here, like bxl.
    // TODO: We also need to now the what the currently checked out revision is and what the latest
    // revision in the repo is to inject into the context (I asked if something like it exists
    // here: https://discord.com/channels/968932220549103686/969291218347524238/1358818101734543380)
    eval.eval_module(ast, &global_builder.build()).unwrap();
    Ok(())
}

#[starlark_module]
fn jj_globals(_global_builder: &mut GlobalsBuilder) {
    // TODO: Move all type instatiantions here
    // TODO: add a commands namespace here which primarily delegates to the existing comands
    // TODO: Starlark needs some Result type, get inspired by bxl and register them here
}

// TODO: move this to a different crate (jj-interpreter, jj-mahou).
mod lang {
    use std::fmt;
    use std::fmt::Debug;
    use std::fmt::Display;
    use std::sync::Arc;

    use allocative::Allocative;
    use itertools::Itertools;
    use jj_lib::backend::BackendResult;
    use jj_lib::backend::Signature;
    use jj_lib::commit::Commit;
    use jj_lib::store::Store;
    use starlark::any::ProvidesStaticType;
    use starlark::environment::GlobalsBuilder;
    use starlark::environment::Methods;
    use starlark::environment::MethodsBuilder;
    use starlark::environment::MethodsStatic;
    use starlark::eval::Evaluator;
    use starlark::starlark_module;
    use starlark::starlark_simple_value;
    use starlark::values::starlark_value;
    use starlark::values::starlark_value_as_type::StarlarkValueAsType;
    use starlark::values::Heap;
    use starlark::values::NoSerialize;
    use starlark::values::StarlarkValue;
    use starlark::values::StringValue;
    use starlark::values::UnpackValue;
    use starlark::values::Value;
    use starlark::values::ValueTyped;
    // use crate::diff_util;

    /// The base information we inject into Starlark.
    #[derive(Allocative, Debug, Clone, ProvidesStaticType)]
    struct JujutsuContext {
        #[allocative(skip)]
        store: Arc<Store>,
        // current_commit: Commit,
        // current_tip: Commit
    }

    impl JujutsuContext<'_> {
        pub fn new() -> Self {}
    }
    // TODO: Integrate with the template language here
    /// A Revision is a single commit in starlark.
    #[derive(Allocative, Debug, PartialEq, ProvidesStaticType, NoSerialize)]
    pub(crate) struct Revision(#[allocative(skip)] Commit);

    impl Revision {
        fn new(commit: Commit) -> Self {
            Self(commit)
        }
    }

    impl Display for Revision {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.fmt(f)
        }
    }

    starlark_simple_value!(Revision);

    #[starlark_module]
    fn revision_methods(builder: &mut MethodsBuilder) {
        /// Get the author of this revision
        fn author<'v>(this: ValueTyped<'v, Revision>) -> starlark::Result<Author> {
            let inner = this.0.clone();
            Ok(Author::new(inner.author().clone()))
        }

        /// Get all modified files in this revision
        fn files<'v>(this: ValueTyped<'v, Revision>) -> starlark::Result<FileSet> {
            let inner = this.0.clone();

            todo!()
        }

        /// Get the parents of the current revision
        fn parents<'v>(this: ValueTyped<'v, Revision>) -> starlark::Result<Vec<Revision>> {
            let parents = this
                .0
                .parents()
                .map(|p| Revision::new(p.unwrap()))
                .collect_vec();
            Ok(parents)
        }

        /// Get the diff of the current revision
        fn diff<'v>(this: ValueTyped<'v, Revision>) -> starlark::Result<Diff> {
            let _ = this.0.clone();
            todo!()
        }

        /// Return the first line of the Revisions description.
        fn subject<'v>(this: ValueTyped<'v, Revision>) -> starlark::Result<StringValue<'v>> {
            let inner = this.0.clone();
            let subject = String::new();
            // let subject = diff_util::DiffRenderer(inner.description());
            Ok(subject.into())
        }

        /// Get the Revisions full description.
        fn description<'v>(
            this: ValueTyped<'v, Revision>,
            heap: &'v Heap,
        ) -> starlark::Result<StringValue<'v>> {
            let inner = this.0.clone();
            Ok(heap.alloc_str(inner.description()))
        }
    }

    #[starlark_value(type = "Revision")]
    impl<'v> StarlarkValue<'v> for Revision {
        fn get_methods() -> Option<&'static Methods> {
            static RES: MethodsStatic = MethodsStatic::new();
            RES.methods(revision_methods)
        }
    }

    #[starlark_module]
    fn rev_globals(globals: &mut GlobalsBuilder) {
        /// A Revision represents the current state of a Change
        ///
        /// Usage
        /// ```python,ignore
        /// def f(rev: Revision): pass
        /// ```
        const Revision: StarlarkValueAsType<Revision> = StarlarkValueAsType::new();
    }

    // TODO: this should be the current repo
    #[derive(Allocative, Debug, ProvidesStaticType, NoSerialize)]
    pub(crate) struct Repo {
        #[allocative(skip)]
        pub(crate) store: Arc<Store>,
        // current_tip: Commit
        // current_commit: Commit
    }

    impl Repo {
        const SUPPORTED_ATTRS: &[&'static str] = &["tip", "current_commit", "user"];
        pub fn new(store: Arc<Store>) -> Self {
            Self { store }
        }

        pub fn new_from_context(context: &JujutsuContext<'_>) -> Self {
            let store = context.store.clone();
            Self { store }
        }
    }

    impl fmt::Display for Repo {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.store.fmt(f)
        }
    }

    #[starlark_value(type = "Repository")]
    impl<'v> StarlarkValue<'v> for Repo {
        fn get_methods() -> Option<&'static Methods> {
            static RES: MethodsStatic = MethodsStatic::new();
            RES.methods(repo_methods)
        }

        fn get_attr(&self, _attr_name: &str, _heap: &'v Heap) -> Option<Value<'v>> {
            Some(Value::new_none())
        }

        fn has_attr(&self, _attr_name: &str, _heap: &'v Heap) -> bool {
            true
        }

        fn dir_attr(&self) -> Vec<String> {
            Self::SUPPORTED_ATTRS
                .iter()
                .map(|s| s.to_string())
                .collect()
        }
    }

    #[starlark_module]
    fn repo_methods(globals: &mut MethodsBuilder) {
        /// Search for all commits with the given author
        fn query_author<'v>(
            this: ValueTyped<'v, Repo>,
            #[starlark(require = named)] _author: Author,
            #[starlark(require = named, default = 10)] _limit: i64,
        ) -> starlark::Result<Vec<Revision>> {
            let _ = this;
            todo!()
        }

        /// Search for all commits which touched these files
        fn query_files<'v>(
            this: ValueTyped<'v, Repo>,
            #[starlark(require = named)] _files: FileSet,
            #[starlark(require = named, default = 10)] _limit: i64,
        ) -> starlark::Result<Vec<Revision>> {
            let _ = this;
            todo!()
        }
    }

    starlark_simple_value!(Repo);

    /// Represents the author field on a commit in the scripting context.
    // TODO: Split Signature from the Author field (e.g author.signature should work).
    #[derive(Allocative, Debug, Clone, ProvidesStaticType, NoSerialize)]
    pub(crate) struct Author(#[allocative(skip)] Signature);

    impl Display for Author {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{:?}", self.0)
        }
    }

    impl<'v> UnpackValue<'v> for Author {
        type Error = starlark::Error;
        fn unpack_value_impl(_value: Value<'v>) -> Result<Option<Self>, Self::Error> {
            Ok(None)
        }
    }

    starlark_simple_value!(Author);

    impl Author {
        fn new(signature: Signature) -> Self {
            Self(signature)
        }
    }

    #[starlark_value(type = "Author")]
    impl<'v> StarlarkValue<'v> for Author {}

    #[starlark_module]
    fn author_methods(builder: &mut MethodsBuilder) {
        /// Return the Authors domain.
        fn domain<'v>(this: ValueTyped<'v, Author>) -> starlark::Result<StringValue<'v>> {
            let _ = this.0.clone();
            todo!()
        }

        fn local<'v>(this: ValueTyped<'v, Author>) -> starlark::Result<StringValue<'v>> {}
    }

    /// Represents a Diff
    // TODO: jj_lib equivalent
    pub(crate) struct Diff;

    /// Represents a FileSet, so something like `glob:"*.rs"
    pub(crate) struct FileSet;
}
