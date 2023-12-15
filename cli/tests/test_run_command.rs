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
//

use crate::common::TestEnvironment;

#[test]
fn test_simple_run_invocation() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");
    let _ = test_env.jj_cmd_ok(
        &repo_path,
        &[
            "run",
            "'fake-formatter --uppercase'",
            "-r",
            "'mutable()..@'",
        ],
    );
}

#[test]
fn test_run_on_immutable() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");
    let _ = test_env.jj_cmd_ok(
        &repo_path,
        &[
            "run",
            "'fake-formatter --uppercase'",
            "-r",
            "'mutable()..@'",
        ],
    );
}

#[test]
fn test_run_noop() {
    let test_env = TestEnvironment::default();
    test_env.jj_cmd_ok(test_env.env_root(), &["git", "init", "repo"]);
    let repo_path = test_env.env_root().join("repo");
    let _ = test_env.jj_cmd_ok(
        &repo_path,
        &[
            "run",
            "'fake-formatter --uppercase'",
            "-r",
            "'mutable()..@'",
        ],
    );
}
