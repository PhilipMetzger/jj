// Copyright 2024 The Jujutsu Authors
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
//! Contains the internals for the movement commands, `next` and `prev`.
//! And additional helpers such as [`choose_commit`]

use std::io::Write;

use jj_lib::commit::Commit;
use jj_lib::revset::Revset;

use crate::cli_util::WorkspaceCommandHelper;
use crate::command_error::{user_error, CommandError};
use crate::ui::Ui;

/// Display a option of choices of `commits`, which you can use
pub fn choose_commit<'a>(
    ui: &mut Ui,
    workspace_command: &WorkspaceCommandHelper,
    cmd: &str,
    commits: &'a [Commit],
) -> Result<&'a Commit, CommandError> {
    writeln!(ui.stdout(), "ambiguous {cmd} commit, choose one to target:")?;
    let mut formatter = ui.stdout_formatter();
    let template = workspace_command.commit_summary_template();
    let mut choices: Vec<String> = Default::default();
    for (i, commit) in commits.iter().enumerate() {
        write!(formatter, "{}: ", i + 1)?;
        template.format(commit, formatter.as_mut())?;
        writeln!(formatter)?;
        choices.push(format!("{}", i + 1));
    }
    writeln!(formatter, "q: quit the prompt")?;
    choices.push("q".to_string());
    drop(formatter);

    let choice = ui.prompt_choice(
        "enter the index of the commit you want to target",
        &choices,
        None,
    )?;
    if choice == "q" {
        return Err(user_error("ambiguous target commit"));
    }

    Ok(&commits[choice.parse::<usize>().unwrap() - 1])
}

/// Describes in which direction to move.
pub enum Direction {
    /// Move to ancestors, so parents and their ancestors.
    Ancestors,
    /// Move to children and their descendants.
    Descendants,
}

pub fn move_direction(amount: i64, direction: Direction, revset: Revset) -> Commit {}
