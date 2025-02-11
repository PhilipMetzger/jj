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

//! This file contains the no-op `jj script` command

use crate::{cli_util::CommandHelper, command_error::CommandError, ui::Ui};

#[derive(Debug, Clone, clap::Args)]
pub struct ScriptArgs {}

pub fn cmd_script(
    _ui: &mut Ui,
    _command: &CommandHelper,
    _args: &ScriptArgs,
) -> Result<(), CommandError> {
    todo!()
}
