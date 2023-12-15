// Copyright 2020 The Jujutsu Authors
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

//! This file contains the internal implementation of `run`.

use std::collections::HashSet;
use std::fs::File;
use std::io;
use std::ops::Deref;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::mpsc::Sender;

use clap::Command;
use futures::StreamExt;
use itertools::Itertools;
use jj_lib::backend::{CommitId, MergedTreeId, TreeValue};
use jj_lib::commit::Commit;
use jj_lib::dag_walk::topo_order_forward_ok;
use jj_lib::matchers::EverythingMatcher;
use jj_lib::merged_tree::MergedTreeBuilder;
use jj_lib::object_id::ObjectId;
use jj_lib::repo::Repo;
use jj_lib::tree::Tree;
use pollster::FutureExt;
use tokio::runtime::Builder;
use tokio::task::JoinSet;
use tokio::{process, runtime, sync};

use crate::cli_util::{CommandHelper, RevisionArg, WorkspaceCommandTransaction};
use crate::command_error::{user_error, CommandError};
use crate::ui::Ui;

#[derive(Debug, thiserror::Error)]
enum RunError {
    #[error("Couldn't create directory")]
    NoDirectoryCreated,
}

impl From<RunError> for CommandError {
    fn from(value: RunError) -> Self {
        CommandError::new(crate::command_error::CommandErrorKind::Cli, Box::new(value))
    }
}

/// Creates the required directories for a StoredWorkingCopy.
/// Returns a tuple of (`output_dir`, `working_copy` and `state`).
fn create_working_copy_paths(
    path: &PathBuf,
) -> Result<(PathBuf, PathBuf, PathBuf), std::io::Error> {
    let output = path.join("output");
    let working_copy = path.join("working_copy");
    let state = path.join("state");
    std::fs::create_dir(&output)?;
    std::fs::create_dir(&working_copy)?;
    std::fs::create_dir(&state)?;
    Ok((output, working_copy, state))
}

/// Represent a `MergeTreeId` in a way that it may be used as a working-copy
/// name. This makes no stability guarantee, as the format may change at
/// any time.
fn to_wc_name(id: &MergedTreeId) -> String {
    match id {
        MergedTreeId::Legacy(tree_id) => tree_id.hex(),
        MergedTreeId::Merge(tree_ids) => {
            let ids = tree_ids
                .map(|id| id.hex())
                .iter_mut()
                .enumerate()
                .map(|(i, s)| {
                    // Incredibly "smart" way to say, append "-" if the number is odd "+"
                    // otherwise.
                    if i & 1 != 0 {
                        s.push('-');
                    } else {
                        s.push('+');
                    }
                    s.to_owned()
                })
                .collect_vec();
            let mut obfuscated: String = ids.concat();
            // `PATH_MAX` could be a problem for different operating systems, so truncate
            // it.
            if obfuscated.len() >= 255 {
                obfuscated.truncate(200);
            }
            obfuscated
        }
    }
}

fn get_runtime(jobs: usize) -> tokio::runtime::Handle {
    let mut builder = Builder::new_multi_thread();
    if cfg!(watchman) {
        // Watchman requires a multithreaded runtime, so just reuse it.
        return runtime::Handle::current();
    }
    if jobs == 1 {
        builder.max_blocking_threads(1);
    } else {
        builder.max_blocking_threads(jobs);
    }
    let rt = builder.build().unwrap();
    rt.handle().clone()
}

/// A commit stored under `.jj/run/`
// TODO: Create a caching backend, which creates these on a dedicated thread or
// threadpool.
struct StoredCommit {
    /// Obfuscated name for an easier lookup. If a tree/directory its not set
    name: Option<String>,
    /// The respective commit unmodified.
    commit: Commit,
    output_dir: PathBuf,
    working_copy_dir: PathBuf,
    state_dir: PathBuf,
    /// The `stdout` of the commit
    stdout: File,
    /// The `stderr` of the commit
    stderr: File,
}

impl StoredCommit {
    fn new(
        name: Option<String>,
        commit: &Commit,
        output_dir: PathBuf,
        working_copy_dir: PathBuf,
        state_dir: PathBuf,
        stdout: File,
        stderr: File,
    ) -> Self {
        Self {
            name,
            commit: commit.clone(),
            output_dir,
            working_copy_dir,
            state_dir,
            stdout,
            stderr,
        }
    }
}

const BASE_PATH: &str = ".jj/run/default";

fn create_output_files(path: &PathBuf) -> Result<(File, File), io::Error> {
    let _path = path;
    Err(io::Error::last_os_error())
}

fn create_working_copies(commits: &[Commit]) -> Result<Vec<StoredCommit>, io::Error> {
    let mut results = vec![];
    for commit in commits {
        let name = to_wc_name(commit.tree_id());
        let base_path = PathBuf::new();
        let (output_dir, working_copy_dir, state_dir) = create_working_copy_paths(&base_path)?;
        let (stdout, stderr) = create_output_files(&base_path)?;

        let stored_commit = StoredCommit::new(
            Some(name),
            commit,
            output_dir,
            working_copy_dir,
            state_dir,
            stdout,
            stderr,
        );
        results.push(stored_commit);
    }
    Ok(results)
}
/// The result of a single command invocation in `run_inner`.
enum RunJobResult {
    /// A `Tree` and it's rewritten `CommitId`
    Success {
        /// The old `CommitId` of the commit.
        old_id: CommitId,
        /// The new `CommitId` for the commit.
        rewritten_id: CommitId,
        /// The new tree generated from the commit.
        new_tree: Tree,
    },
    /// The commands exit code
    // TODO: use an actual error here.
    Failure(ExitStatus),
}

// TODO: make this more revset stream friendly.
async fn run_inner<'a>(
    tx: WorkspaceCommandTransaction<'a>,
    sender: Sender<RunJobResult>,
    jobs: usize,
    shell_command: &str,
    commits: &[StoredCommit],
) -> Result<(), RunError> {
    let mut command_futures = JoinSet::new();
    for commit in commits {
        command_futures.spawn(rewrite_commit(tx, commit.deref(), shell_command));
    }

    while let Some(res) = command_futures.join_next().await {
        let done = res?;
        sender.send(done?);
    }
    Ok(())
}

/// Rewrite a single `StoredCommit`.
async fn rewrite_commit<'a>(
    tx: WorkspaceCommandTransaction<'a>,
    stored_commit: &StoredCommit,
    shell_command: &str,
) -> Result<RunJobResult, RunError> {
    let mut command_builder = tokio::process::Command::new("sh")
        .args([shell_command])
        // TODO: relativize
        // .env("JJ_PATH", stored_commit.tree_path)
        .stdout(stored_commit.stdout)
        .stderr(stored_commit.stderr);
    let status = command_builder.status().await;
    let mut paths = vec![];
    let mut file_ids = HashSet::new();
    // Paths modified in parent commits in the set should also be updated in this
    // commit
    let commit = stored_commit.commit;
    for parent_id in commit.parent_ids() {
        if let Some(parent_paths) = commit_paths.get(parent_id) {
            paths.extend_from_slice(parent_paths);
        }
    }
    let parent_tree = commit.parent_tree(tx.repo())?;
    let tree = commit.tree()?;
    let mut diff_stream = parent_tree.diff_stream(&tree, &EverythingMatcher);
    while let Some((repo_path, diff)) = diff_stream.next().await {
        let (_before, after) = diff?;
        for term in after.into_iter().flatten() {
            if let TreeValue::File { id, executable: _ } = term {
                file_ids.insert((repo_path.clone(), id));
                paths.push(repo_path.clone());
            }
        }
    }

    Ok(RunJobResult::Success {
        old_id: (),
        rewritten_id: (),
        new_tree: (),
    })
}

/// Run a command across a set of revisions.
///
///
/// All recorded state will be persisted in the `.jj` directory, so occasionally
/// a `jj run --clean` is needed to clean up disk space.
///
/// # Example
///
/// # Run pre-commit on your local work
/// $ jj run 'pre-commit run .github/pre-commit.yaml' -r (trunk()..@) -j 4
///
/// This allows pre-commit integration and other funny stuff.
#[derive(clap::Args, Clone, Debug)]
#[command(verbatim_doc_comment)]
pub struct RunArgs {
    /// The command to run across all selected revisions.
    shell_command: String,
    /// The revisions to change.
    #[arg(long, short, default_value = "@")]
    revisions: RevisionArg,
    /// A no-op option to match the interface of `git rebase -x`.
    #[arg(short = 'x', hide = true)]
    unused_command: bool,
    /// How many processes should run in parallel, uses by default all cores.
    #[arg(long, short)]
    jobs: Option<usize>,
}

pub fn cmd_run(ui: &mut Ui, command: &CommandHelper, args: &RunArgs) -> Result<(), CommandError> {
    let workspace_command = command.workspace_helper(ui)?;
    let resolved_commits: Vec<_> = workspace_command
        .parse_revset(&args.revisions)?
        .evaluate_to_commits()?
        .try_collect()?;
    // Jobs are resolved in this order:
    // 1. Commandline argument iff > 0.
    // 2. the amount of cores available.
    // 3. a single job, if all of the above fails.
    let jobs = match args.jobs {
        Some(0) => return Err(user_error("must pass at least one job")),
        Some(jobs) => Some(jobs),
        None => std::thread::available_parallelism().map(|t| t.into()).ok(),
    }
    // Fallback to a single user-visible job.
    .unwrap_or(1usize);

    let (mut sender_tx, receiver) = std::sync::mpsc::channel();
    // let repo = workspace_command.repo();
    // let cache_backend = repo.working_copy_store();
    // let _wc_copies = cache_backend.get_or_create_stores(_resolved_commits)?;

    // Toposort the commits.
    let topo_sorted_commits = topo_order_forward_ok(
        resolved_commits.to_vec(),
        |c: &Commit| c.id(),
        |c: &Commit| c.parent_ids(),
    )?;
    let stored_commits = create_working_copies(&topo_sorted_commits)?;

    let tx = workspace_command.start_transaction();
    // Start all the jobs.
    async { run_inner(tx, sender_tx, jobs, &args.shell_command, &stored_commits).await? }
        .block_on();

    // Wait until we have all results.
    loop {
        let result = receiver.recv();
        if result.is_err() {
            tracing::debug!("the");
            break;
        }
        match result {
            RunJobResult::Success {
                old_id,
                new_id,
                tree,
            } => {}
            RunJobResult::Failure(err) => {}
        }
    }
    tx.mut_repo().transform_descendants(
        command.settings(),
        root_commits.iter().ids().cloned().collect_vec(),
        |mut rewriter| {
            let paths = commit_paths.get(rewriter.old_commit().id()).unwrap();
            let old_tree = rewriter.old_commit().tree()?;
            let mut tree_builder = MergedTreeBuilder::new(old_tree.id().clone());
            for path in paths {
                let old_value = old_tree.path_value(path);
                let new_value = old_value.map(|old_term| {
                    if let Some(TreeValue::File { id, executable }) = old_term {
                        if let Some(new_id) = formatted.get(&(path, id)) {
                            Some(TreeValue::File {
                                id: new_id.clone(),
                                executable: *executable,
                            })
                        } else {
                            old_term.clone()
                        }
                    } else {
                        old_term.clone()
                    }
                });
                if new_value != old_value {
                    tree_builder.set_or_remove(path.clone(), new_value);
                }
            }
            let new_tree = tree_builder.write_tree(rewriter.mut_repo().store())?;
            let builder = rewriter.reparent(command.settings())?;
            builder.set_tree_id(new_tree).write()?;
            Ok(())
        },
    )?;

    Err(user_error("This is a stub, do not use"))
}
