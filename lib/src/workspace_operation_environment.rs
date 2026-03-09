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

//! Contains the `WorkspaceOperationEnvironment` which holds some internal state
//! about aliases and more.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;

use chrono::TimeZone as _;

use crate::backend::CommitId;
use crate::config::ConfigGetResultExt as _;
use crate::conflicts::ConflictMarkerStyle;
use crate::fileset::FilesetAliasesMap;
use crate::fileset::FilesetParseContext;
use crate::id_prefix::IdPrefixContext;
use crate::ref_name::RemoteName;
use crate::ref_name::WorkspaceName;
use crate::ref_name::WorkspaceNameBuf;
use crate::repo::Repo;
use crate::repo_path::RepoPathUiConverter;
use crate::revset::ResolvedRevsetExpression;
use crate::revset::RevsetAliasesMap;
use crate::revset::RevsetDiagnostics;
use crate::revset::RevsetExpression;
use crate::revset::RevsetExtensions;
use crate::revset::RevsetParseContext;
use crate::revset::RevsetParseError;
use crate::revset::RevsetWorkspaceContext;
use crate::revset::UserRevsetExpression;
use crate::revset::{self};
use crate::settings::UserSettings;
use crate::workspace::Workspace;

/// Metadata and configuration loaded for a specific workspace.
pub struct WorkspaceCommandEnvironment {
    /// The loaded user settings
    settings: UserSettings,
    /// The defined  fileset-aliases
    fileset_aliases_map: FilesetAliasesMap,
    /// The defined revset-aliases
    revset_aliases_map: RevsetAliasesMap,
    /// The path converter used to for messages.
    path_converter: RepoPathUiConverter,
    /// The workspace name the environment was created for.
    workspace_name: WorkspaceNameBuf,
    /// The `immutable_heads()` expression used for this environment.
    immutable_heads_expression: Arc<UserRevsetExpression>,
    /// The short prefixes set defined for this environment.
    short_prefixes_expression: Option<Arc<UserRevsetExpression>>,
    /// The configured `ConflictedMarkerStyle` for this environment.
    conflict_marker_style: ConflictMarkerStyle,
}

impl WorkspaceCommandEnvironment {
    /// Create a new `WorkspaceCommandEnvironment` for the given workspace.
    pub fn new(
        workspace: &Workspace,
        fileset_aliases_map: FilesetAliasesMap,
        revset_aliases_map: RevsetAliasesMap,
        path_converter: RepoPathUiConverter,
        workspace_name: WorkspaceNameBuf,
        conflict_marker_style: ConflictMarkerStyle,
    ) -> Result<Self, ()> {
        let settings = workspace.settings();
        let mut env = Self {
            settings: settings.clone(),
            fileset_aliases_map,
            revset_aliases_map,
            path_converter,
            workspace_name,
            immutable_heads_expression: RevsetExpression::root(),
            short_prefixes_expression: None,
            conflict_marker_style,
        };
        env.reload_revset_expressions()?;
        Ok(env)
    }

    /// Get access to the `RepoPathUiConverter`.
    pub(crate) fn path_converter(&self) -> &RepoPathUiConverter {
        &self.path_converter
    }

    /// Get the name for which this environment was created for.
    pub fn workspace_name(&self) -> &WorkspaceName {
        &self.workspace_name
    }

    /// Parsing context for fileset expressions specified by command arguments.
    pub(crate) fn fileset_parse_context(&self) -> FilesetParseContext<'_> {
        FilesetParseContext {
            aliases_map: &self.fileset_aliases_map,
            path_converter: &self.path_converter,
        }
    }

    /// Parsing context for fileset expressions loaded from config files.
    pub fn fileset_parse_context_for_config(&self) -> FilesetParseContext<'_> {
        // TODO: bump MSRV to 1.91.0 to leverage const PathBuf::new()
        static ROOT_PATH_CONVERTER: LazyLock<RepoPathUiConverter> =
            LazyLock::new(|| RepoPathUiConverter::Fs {
                cwd: PathBuf::new(),
                base: PathBuf::new(),
            });
        FilesetParseContext {
            aliases_map: &self.fileset_aliases_map,
            path_converter: &ROOT_PATH_CONVERTER,
        }
    }

    /// Create a new `RevsetParseContext` for this environment.
    pub fn revset_parse_context<'a>(
        &'a self,
        default_ignored_remote: Option<&'a RemoteName>,
        extensions: &'a RevsetExtensions,
        revsets_use_glob_by_default: bool,
    ) -> RevsetParseContext<'_> {
        let workspace_context = RevsetWorkspaceContext {
            path_converter: &self.path_converter,
            workspace_name: &self.workspace_name,
        };
        let now = if let Some(timestamp) = self.settings.commit_timestamp() {
            chrono::Local
                .timestamp_millis_opt(timestamp.timestamp.0)
                .unwrap()
        } else {
            chrono::Local::now()
        };
        RevsetParseContext {
            aliases_map: &self.revset_aliases_map,
            local_variables: HashMap::new(),
            user_email: self.settings.user_email(),
            date_pattern_context: now.into(),
            default_ignored_remote: default_ignored_remote,
            fileset_aliases_map: &self.fileset_aliases_map,
            use_glob_by_default: revsets_use_glob_by_default,
            extensions,
            workspace: Some(workspace_context),
        }
    }

    /// Creates fresh new context which manages cache of short commit/change ID
    /// prefixes. New context should be created per repo view (or operation.)
    pub fn new_id_prefix_context(&self, extensions: Arc<RevsetExtensions>) -> IdPrefixContext {
        let context = IdPrefixContext::new(extensions.clone());
        match &self.short_prefixes_expression {
            None => context,
            Some(expression) => context.disambiguate_within(expression.clone()),
        }
    }

    /// Updates parsed revset expressions.
    fn reload_revset_expressions(&mut self) -> Result<(), ()> {
        self.immutable_heads_expression = self.load_immutable_heads_expression()?;
        self.short_prefixes_expression = self.load_short_prefixes_expression()?;
        Ok(())
    }

    /// User-configured expression defining the immutable set.
    pub fn immutable_expression(&self) -> Arc<UserRevsetExpression> {
        // Negated ancestors expression `~::(<heads> | root())` is slightly
        // easier to optimize than negated union `~(::<heads> | root())`.
        self.immutable_heads_expression.ancestors()
    }

    /// User-configured expression defining the heads of the immutable set.
    pub fn immutable_heads_expression(&self) -> &Arc<UserRevsetExpression> {
        &self.immutable_heads_expression
    }

    /// User-configured conflict marker style for materializing conflicts
    pub fn conflict_marker_style(&self) -> ConflictMarkerStyle {
        self.conflict_marker_style
    }

    pub fn load_immutable_heads_expression(&self) -> Result<Arc<UserRevsetExpression>, ()> {
        let mut diagnostics = RevsetDiagnostics::new();
        // let expression = revset_util::parse_immutable_heads_expression(
        //     &mut diagnostics,
        //     &self.revset_parse_context(),
        // )
        // .map_err(|e| config_error_with_message("Invalid `revset-aliases.immutable_heads()`", e))?;
        // print_parse_diagnostics(ui, "In `revset-aliases.immutable_heads()`", &diagnostics)?;
        // Ok(expression)
    }

    pub fn load_short_prefixes_expression(
        &self,
    ) -> Result<Option<Arc<UserRevsetExpression>>, RevsetParseError> {
        let revset_string = self
            .settings
            .get_string("revsets.short-prefixes")
            .optional()?
            .map_or_else(|| self.settings.get_string("revsets.log"), Ok)?;
        if revset_string.is_empty() {
            Ok(None)
        } else {
            let mut diagnostics = RevsetDiagnostics::new();
            let expression = revset::parse(
                &mut diagnostics,
                &revset_string,
                &self.revset_parse_context(),
            )?;
            Ok(Some(expression))
        }
    }

    /// Returns first immutable commit.
    pub fn find_immutable_commit(
        &self,
        repo: &dyn Repo,
        to_rewrite_expr: &Arc<ResolvedRevsetExpression>,
        ignore_immutable: bool,
        extensions: Arc<RevsetExtensions>,
    ) -> Result<Option<CommitId>, ()> {
        let immutable_expression = if ignore_immutable {
            UserRevsetExpression::root()
        } else {
            self.immutable_expression()
        };

        // Not using self.id_prefix_context() because the disambiguation data
        // must not be calculated and cached against arbitrary repo. It's also
        // unlikely that the immutable expression contains short hashes.
        let id_prefix_context = IdPrefixContext::new(extensions.clone());
        let immutable_expr = RevsetExpressionEvaluator::new(
            repo,
            extensions.clone(),
            &id_prefix_context,
            immutable_expression,
        )
        .resolve()
        .map_err(|e| config_error_with_message("Invalid `revset-aliases.immutable_heads()`", e))?;

        let mut commit_id_iter = immutable_expr
            .intersection(to_rewrite_expr)
            .evaluate(repo)?
            .iter();
        Ok(commit_id_iter.next().transpose()?)
    }
}
