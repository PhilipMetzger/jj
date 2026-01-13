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

mod builtin;
mod interpreter;

use std::collections::HashSet;
use std::fmt;
use std::fmt::Debug;
use std::fmt::Display;
use std::sync::Arc;

use allocative::Allocative;
use itertools::Itertools;
use jj_lib::backend::BackendResult;
use jj_lib::backend::Signature;
use jj_lib::commit::Commit;
use jj_lib::settings::UserSettings;
use jj_lib::store::Store;
use starlark::any::ProvidesStaticType;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Methods;
use starlark::environment::MethodsBuilder;
use starlark::environment::MethodsStatic;
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::Heap;
use starlark::values::NoSerialize;
use starlark::values::StarlarkValue;
use starlark::values::StringValue;
use starlark::values::UnpackValue;
use starlark::values::Value;
use starlark::values::ValueTyped;
use starlark::values::starlark_value;
use starlark::values::starlark_value_as_type::StarlarkValueAsType;
use tokio::runtime;
use tokio::runtime::Handle;

// TODO: move parts of it to the library use
// crate::cli_util::WorkspaceCommandHelper; ditto use crate::diff_util;

/// The base information we inject into Starlark.
#[derive(Allocative, Debug, Clone, ProvidesStaticType)]
struct JujutsuContext {
    store: Arc<Store>,
    config: Arc<UserSettings>,
    #[allocative(skip)] // TODO: maybe fix upstream?
    runtime_handle: Handle,
    // The workspace helper is needed to rewrite commits and more
    // TODO: consider giving users access to build transactions
    // workspace_helper: &'v WorkspaceCommandHelper
    // current_commit: Option<Commit>,
    // current_tip: Commit
}

#[starlark_module]
fn jj_globals(_global_builder: &mut GlobalsBuilder) {
    // TODO: Move all type instatiantions here
    // TODO: add a commands namespace here which primarily delegates to the
    // existing comands TODO: Starlark needs some Result type, get inspired
    // by bxl and register them here
}

impl JujutsuContext {
    
    pub fn new(user_settings: UserSettings, store: Arc<Store>, runtime_handle: Handle) -> Self {
        Self {
            store,
            config: Arc::new(user_settings),
            runtime_handle
            
        }
    }

    pub fn edit_commit<'v>(&'v mut self, commit: Revision, new_desc: Option<&str>) {}
}

// TODO: Integrate with the template language here
/// A Revision is a single commit in starlark.
#[derive(Allocative, Debug, derive_more::Display, PartialEq, ProvidesStaticType, NoSerialize)]
#[display("{:?}", _0)]
pub(crate) struct Revision(Commit);

impl Revision {
    fn new(commit: Commit) -> Self {
        Self(commit)
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
        let subject = inner.description().lines()
        // let subject = diff_util::DiffRenderer(inner.description());
        Ok(subject.into())
    }

    /// Get the Revisions full description.
    #[starlark(attribute)]
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

    pub fn new_from_context(context: &JujutsuContext) -> Self {
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
#[derive(Allocative, Debug, Clone, ProvidesStaticType, NoSerialize)]
pub(crate) struct Author(Signature);

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
// TODO: find jj_lib equivalent
pub(crate) struct Diff;

/// Represents a FileSet, so something like `glob:"*.rs"
#[derive(Allocative, Debug, NoSerialize)]
pub(crate) struct FileSet;

// starlark_simple_value!(FileSet);


/// A `CliArg` is used to parse Starlark values from Clap. Heavily inspired from
/// buck2's bxl.
pub(crate) struct CliArg {
    /// The actual representation of the argument which is used for Clap.
    inner: CliArgType,
    /// The associated documentation, if there's one.
    doc: Option<&'static str>,
    /// The Clap short option if specified.
    short: Option<char>,
}

/// All the supported types we support from the CLI.
// TODO: pub(crate) use all the things from cli_util.rs
#[derive(Default, Debug)]
enum CliArgType {
    #[default]
    None,
    Bool,
    Int,
    String,
    Revision,
    Enum(Arc<HashSet<String>>),
    List(Arc<CliArgType>),
    Option(Arc<CliArgType>),
}

impl CliArgType {
    pub(crate) fn bool() -> Self {
        CliArgType::Bool
    }

    pub(crate) fn int() -> Self {
        CliArgType::Int
    }

    pub(crate) fn string() -> Self {
        CliArgType::String
    }

    pub(crate) fn revision() -> Self {
        CliArgType::Revision
    }

    pub(crate) fn r#enum(inner: HashSet<String>) -> Self {
        CliArgType::Enum(Arc::new(inner))
    }

    pub(crate) fn list(inner: CliArgType) -> Self {
        Self::List(Arc::new(inner))
    }
    
    pub(crate) fn option(inner: CliArgType) -> Self {
        Self::Option(Arc::new(inner))
    }
}

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}
