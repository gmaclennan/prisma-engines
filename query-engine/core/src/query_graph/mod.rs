///! Query graph abstraction for simple high-level query representation
///! and manipulation.
mod guard;
mod formatters;
mod transformers;
mod error;

pub use error::*;
pub use formatters::*;
pub use transformers::*;

use connector::*;
use guard::*;
use petgraph::{graph::*, visit::EdgeRef as PEdgeRef, *};
use prisma_models::PrismaValue;
use std::borrow::Borrow;

/// Implementation detail of the QueryGraph.
type InnerGraph = Graph<Guard<Node>, Guard<QueryGraphDependency>>;

pub enum Node {
    Query(Query),
    Flow(Flow),
}

impl From<Query> for Node {
    fn from(q: Query) -> Node {
        Node::Query(q)
    }
}

impl From<Flow> for Node {
    fn from(f: Flow) -> Node {
        Node::Flow(f)
    }
}

pub enum Flow {
    /// Expresses a conditional control flow in the graph.
    /// Currently, the evaluation function receives the parent ID as PrismaValue if it exists, None otherwise.
    /// Possible outgoing edges are `then` and `else`, each at most once, with `then` required to be present.
    If(Box<dyn FnOnce(Option<PrismaValue>) -> bool>),
}

#[derive(Default)]
pub struct QueryGraph {
    graph: InnerGraph,

    /// Designates the nodes that are returning the result of the entire QueryGraph.
    /// If no nodes are set, the interpretation will take the result of the
    /// last statement derived from the graph.
    result_nodes: Vec<NodeIndex>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeRef {
    node_ix: NodeIndex,
}

impl NodeRef {
    /// Returns the unique identifier of the Node.
    pub fn id(&self) -> String {
        self.node_ix.index().to_string()
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeRef {
    edge_ix: EdgeIndex,
}

/// Stored on the edges of the QueryGraph, a QueryGraphDependency contains information on how children are connected to their parents,
/// expressing for example the need for additional information from the parent to be able to execute at runtime.
pub enum QueryGraphDependency {
    /// Simple dependency indicating order of execution. Effectively a NOOP for now.
    ExecutionOrder,

    /// Performs a transformation on a node based on the parent ID (PrismaValue).
    ParentId(Box<dyn FnOnce(Node, PrismaValue) -> Node>),

    /// Only valid in the context of a `If` control flow node.
    Then,

    /// Only valid in the context of a `If` control flow node.
    Else,
}

impl QueryGraph {
    pub fn new() -> Self {
        Self {
            graph: InnerGraph::new(),
            result_nodes: vec![],
        }
    }

    /// Adds a result node to the graph.
    pub fn add_result_node(&mut self, node: &NodeRef) {
        self.result_nodes.push(node.node_ix.clone());
    }

    /// Checks if the given node is marked as one of the result nodes in the graph.
    pub fn is_result_node(&self, node: &NodeRef) -> bool {
        self.result_nodes
            .iter()
            .find(|rn| rn.index() == node.node_ix.index())
            .is_some()
    }

    /// Checks if the subgraph starting at the given node contains the node designated as the overall result.
    pub fn subgraph_contains_result(&self, node: &NodeRef) -> bool {
        self.is_result_node(node)
            || self
                .outgoing_edges(node)
                .into_iter()
                .find(|edge| {
                    let child_node = self.edge_target(edge);
                    self.subgraph_contains_result(&child_node)
                })
                .is_some()
    }

    pub fn root_nodes(&self) -> Vec<NodeRef> {
        let graph = self.graph.borrow();

        graph
            .node_indices()
            .filter_map(|ix| {
                if let Some(_) = graph.edges_directed(ix, Direction::Incoming).next() {
                    None
                } else {
                    Some(ix)
                }
            })
            .map(|node_ix: NodeIndex| NodeRef { node_ix })
            .collect()
    }

    pub fn create_node<T>(&mut self, t: T) -> NodeRef where T: Into<Node> {
        let node_ix = self.graph.add_node(Guard::new(t.into()));

        NodeRef { node_ix }
    }

    pub fn create_edge(&mut self, from: &NodeRef, to: &NodeRef, content: QueryGraphDependency) -> EdgeRef {
        let edge_ix = self.graph.add_edge(from.node_ix, to.node_ix, Guard::new(content));

        EdgeRef { edge_ix }
    }

    pub fn node_content(&self, node: &NodeRef) -> Option<&Node> {
        self.graph.node_weight(node.node_ix).unwrap().borrow()
    }

    pub fn edge_content(&self, edge: &EdgeRef) -> Option<&QueryGraphDependency> {
        self.graph.edge_weight(edge.edge_ix).unwrap().borrow()
    }

    pub fn edge_source(&self, edge: &EdgeRef) -> NodeRef {
        let (node_ix, _) = self.graph.edge_endpoints(edge.edge_ix).unwrap();

        NodeRef { node_ix }
    }

    pub fn edge_target(&self, edge: &EdgeRef) -> NodeRef {
        let (_, node_ix) = self.graph.edge_endpoints(edge.edge_ix).unwrap();

        NodeRef { node_ix }
    }

    pub fn outgoing_edges(&self, node: &NodeRef) -> Vec<EdgeRef> {
        self.collect_edges(node, Direction::Outgoing)
    }

    pub fn incoming_edges(&self, node: &NodeRef) -> Vec<EdgeRef> {
        self.collect_edges(node, Direction::Incoming)
    }

    /// Removes the edge from the graph but leaves the graph intact by keeping the empty
    /// edge in the graph by plucking the content of the edge, but not the edge itself.
    pub fn pluck_edge(&mut self, edge: EdgeRef) -> QueryGraphDependency {
        self.graph.edge_weight_mut(edge.edge_ix).unwrap().unset()
    }

    /// Removes the node from the graph but leaves the graph intact by keeping the empty
    /// node in the graph by plucking the content of the node, but not the node itself.
    pub fn pluck_node(&mut self, node: NodeRef) -> Node {
        self.graph.node_weight_mut(node.node_ix).unwrap().unset()
    }

    /// Completely removes the edge from the graph, returning it's content.
    pub fn remove_edge(&mut self, edge: EdgeRef) -> Option<QueryGraphDependency> {
        self.graph.remove_edge(edge.edge_ix).unwrap().into_inner()
    }

    pub fn is_direct_child(&self, parent: &NodeRef, child: &NodeRef) -> bool {
        self.incoming_edges(child)
            .into_iter()
            .find(|edge| &self.edge_source(edge) != parent)
            .is_none()
    }

    fn collect_edges(&self, node: &NodeRef, direction: Direction) -> Vec<EdgeRef> {
        let mut edges = self
            .graph
            .edges_directed(node.node_ix, direction)
            .map(|edge| EdgeRef { edge_ix: edge.id() })
            .collect::<Vec<_>>();

        edges.sort();
        edges
    }
}