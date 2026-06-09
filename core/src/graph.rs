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

//! Contains a `GraphNode` helper which helps discovering direct edges to revisions.

/// Node and edges pair of type `N` and `ID` respectively.
///
/// `ID` uniquely identifies a node within the graph. It's usually cheap to
/// clone. There should be a pure `(&N) -> &ID` function.
pub type GraphNode<N, ID = N> = (N, Vec<GraphEdge<ID>>);

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub struct GraphEdge<N> {
    pub target: N,
    pub edge_type: GraphEdgeType,
}

impl<N> GraphEdge<N> {
    pub fn missing(target: N) -> Self {
        Self {
            target,
            edge_type: GraphEdgeType::Missing,
        }
    }

    pub fn direct(target: N) -> Self {
        Self {
            target,
            edge_type: GraphEdgeType::Direct,
        }
    }

    pub fn indirect(target: N) -> Self {
        Self {
            target,
            edge_type: GraphEdgeType::Indirect,
        }
    }

    pub fn map<M>(self, f: impl FnOnce(N) -> M) -> GraphEdge<M> {
        GraphEdge {
            target: f(self.target),
            edge_type: self.edge_type,
        }
    }

    pub fn is_missing(&self) -> bool {
        self.edge_type == GraphEdgeType::Missing
    }

    pub fn is_direct(&self) -> bool {
        self.edge_type == GraphEdgeType::Direct
    }

    pub fn is_indirect(&self) -> bool {
        self.edge_type == GraphEdgeType::Indirect
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum GraphEdgeType {
    Missing,
    Direct,
    Indirect,
}
