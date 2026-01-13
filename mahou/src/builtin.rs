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

//! Builtin types for Mahou.

use starlark::environment::GlobalsBuilder;
use starlark::starlark_module;
use starlark::values::starlark_value_as_type::StarlarkValueAsType;

pub(crate) mod context;
pub(crate) mod fileset;
pub(crate) mod mahou_main;
pub(crate) mod promise;
pub(crate) mod repository;
pub(crate) mod revision;

#[starlark_module]
pub(crate) fn register_jj_types(_globals: &mut GlobalsBuilder) {
    /// The jj.Context type passed to the mahou.main(...) function.
    ///
    /// Usage
    /// ```python,ignore
    /// def _main(jj.context Context): pass
    ///
    /// name = mahou.main(
    ///   impl = _main,
    /// #...
    /// )
    /// ```
    const Context: StarlarkValueAsType<Context> = StarlarkValueAsType::new();
    /// A Revision represents the current state of a Change
    ///
    /// Usage
    /// ```python,ignore
    /// def f(rev: Revision): pass
    /// ```
    const Revision: StarlarkValueAsType<Revision> = StarlarkValueAsType::new();
}
