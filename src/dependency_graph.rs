use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use oca_rs::facade::build::References;
use petgraph::{
    algo::toposort, graph::NodeIndex, graphmap::DiGraphMap, visit::depth_first_search, Graph,
};
use regex::Regex;
use said::SelfAddressingIdentifier;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum GraphError {
    #[error("Cycle detected")]
    Cycle,
    #[error("Unknown name: {0}")]
    UnknownRefn(String),
    #[error("Unknown said for name {0}")]
    UnknownSaid(String),
    #[error(transparent)]
    NodeParsingError(#[from] NodeParsingError),
}

#[derive(Error, Debug, Clone)]
pub enum NodeParsingError {
    #[error("File parsing error: {0}")]
    FileParsing(String),
    #[error("OCA file doesn't contain bundle name: {0}")]
    MissingRefn(PathBuf),
}

#[derive(Default, Debug, Clone)]
pub struct Node {
    pub refn: String,
    pub path: PathBuf,
    pub said: Option<SelfAddressingIdentifier>,
}

pub struct DependencyGraph {
    base_dir: PathBuf,
    graph: Graph<Node, ()>,
}

impl DependencyGraph {
    pub fn from_paths<I, P>(base_dir: &Path, file_paths: I) -> Result<Self, GraphError>
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        // Helper variable for saving connections between nodes, before it was
        // added to graph. Key is refn, and values are indices of nodes, that
        // should have connection with node of given refn.
        let mut edges_to_save = HashMap::new();
        let mut graph = DependencyGraph {
            base_dir: base_dir.to_path_buf(),
            graph: Graph::<Node, ()>::new(),
        };
        file_paths
            .into_iter()
            // Files without refn are ignored
            .filter_map(|path| parse_node(base_dir, path.as_ref()).ok())
            .for_each(|(node, dependencies)| {
                let index = graph.insert_node(node, &mut edges_to_save);
                for dep in dependencies {
                    let edges = edges_to_save.get_mut(&dep);
                    match edges {
                        Some(edges) => {
                            edges.push(index);
                        }
                        None => {
                            edges_to_save.insert(dep.clone(), vec![index]);
                        }
                    };
                }
            });

        // Process remaining edges.
        for (refn, nodes) in edges_to_save.iter() {
            let ind = graph.get_index(refn);
            if let Ok(ind) = ind {
                let edges = nodes.iter().map(|n| (n.to_owned(), ind));
                graph.graph.extend_with_edges(edges);
            }
        }
        Ok(graph)
    }

    pub fn sort(&self) -> Result<Vec<Node>, GraphError> {
        let sorted = toposort(&self.graph, None).map_err(|_e| GraphError::Cycle)?;
        Ok(sorted
            .into_iter()
            .rev()
            .map(|i| self.graph[i].clone())
            .collect())
    }

    pub fn get_index(&self, refn: &str) -> Result<NodeIndex, GraphError> {
        self.graph
            .node_indices()
            .find(|id| self.graph[*id].refn.eq(&refn))
            .ok_or(GraphError::UnknownRefn(refn.to_owned()))
    }

    pub fn neighbors(&self, refn: &str) -> Result<Vec<Node>, GraphError> {
        let index = self.get_index(refn)?;
        Ok(self
            .graph
            .neighbors(index)
            .map(|n| self.graph[n].clone())
            .collect::<Vec<_>>())
    }

    pub fn oca_file_path(&self, refn: &str) -> Result<PathBuf, GraphError> {
        let index = self.get_index(refn)?;
        let relative_path = self.graph[index].path.clone();
        let mut path = self.base_dir.clone();
        path.push(relative_path);
        Ok(path)
    }

    pub fn get_said(&self, refn: &str) -> Result<SelfAddressingIdentifier, GraphError> {
        let i = self.get_index(refn)?;
        let node = &self.graph[i];
        node.said
            .clone()
            .ok_or(GraphError::UnknownSaid(refn.to_string()))
    }
}

impl DependencyGraph {
    /// Adds node to graph. If `edges_to_save` contains edges corresponding to
    /// node refn, graph will be updated.
    fn insert_node(
        &mut self,
        node: Node,
        edges_to_save: &mut HashMap<String, Vec<NodeIndex>>,
    ) -> NodeIndex {
        let refn = node.refn.clone();
        let index = self.graph.add_node(node);
        if let Some(edges) = edges_to_save.remove(&refn) {
            for edge in edges {
                self.graph.add_edge(edge, index, ());
            }
        }
        index
    }

    pub fn update_said(
        &mut self,
        refn: &str,
        value: SelfAddressingIdentifier,
    ) -> Result<(), GraphError> {
        let i = self.get_index(refn)?;
        let node = self.graph.node_weight_mut(i).unwrap();
        node.said = Some(value);
        Ok(())
    }

    pub fn update_refn(&mut self, refn: &str, new_refn: String) -> Result<(), GraphError> {
        let i = self.get_index(refn)?;
        let node = self.graph.node_weight_mut(i).unwrap();
        node.refn = new_refn;
        Ok(())
    }

    fn find_refn(lines: &[String]) -> Vec<String> {
        let re = Regex::new(r"refn:([^\s\]]+)").expect("Invalid regex");
        let mut refn = Vec::new();

        for line in lines {
            for cap in re.captures_iter(line) {
                if let Some(matched) = cap.get(1) {
                    refn.push(matched.as_str().to_string());
                }
            }
        }
        refn
    }
}

pub fn parse_node(base: &Path, file_path: &Path) -> Result<(Node, Vec<String>), NodeParsingError> {
    let (name, lines) = parse_name(file_path)?;
    match name {
        Some(name_part) => {
            let ref_name = name_part.trim_matches('"').to_string();
            let ref_node = Node {
                refn: ref_name,
                path: file_path.strip_prefix(base).unwrap().into(),
                said: None,
            };
            Ok((ref_node, DependencyGraph::find_refn(&lines)))
        }
        None => Err(NodeParsingError::MissingRefn(file_path.to_owned())),
    }
}

pub fn parse_name(file_path: &Path) -> Result<(Option<String>, Vec<String>), NodeParsingError> {
    let content = fs::read_to_string(file_path)
        .map_err(|_e| NodeParsingError::FileParsing("Failed to read file".to_string()))?;
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let ref_name_line = lines
        .first()
        .ok_or(NodeParsingError::FileParsing("File is empty".to_string()))?;
    let name = ref_name_line.split("name=").nth(1).map(|n| n.to_string());
    Ok((name, lines))
}

#[derive(Clone)]
pub struct MutableGraph {
    pub graph: Arc<Mutex<DependencyGraph>>,
}

impl MutableGraph {
    pub fn new<I, P>(base_dir: &Path, file_paths: I) -> Self
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        let g = DependencyGraph::from_paths(base_dir, file_paths).unwrap();
        Self {
            graph: Arc::new(Mutex::new(g)),
        }
    }

    pub fn sort(&self) -> Result<Vec<Node>, GraphError> {
        let g = self.graph.lock().unwrap();
        g.sort()
    }

    pub fn oca_file_path(&self, refn: &str) -> Result<PathBuf, GraphError> {
        let g = self.graph.lock().unwrap();
        g.oca_file_path(refn)
    }

    pub fn update_refn(&self, refn: &str, new_refn: String) -> Result<(), GraphError> {
        let mut g = self.graph.lock().unwrap();
        g.update_refn(refn, new_refn)
    }

    pub fn node(&self, refn: &str) -> Result<Node, GraphError> {
        let g = self.graph.lock().unwrap();
        let start_node = g.get_index(refn)?;
        Ok(g.graph[start_node].clone())
    }

     pub fn get_ancestors(&self, refn: &str) -> Result<Vec<Node>, GraphError> {
        let g = self.graph.lock().unwrap();
        let start_node = g.get_index(refn)?;
        let mut h = DiGraphMap::new();
        let mut rev_graph = g.graph.clone();
        rev_graph.reverse();
        depth_first_search(&rev_graph, Some(start_node), |event| {
            use petgraph::visit::DfsEvent::*;
            match event {
                CrossForwardEdge(parent, child)
                | BackEdge(parent, child)
                | TreeEdge(parent, child) 
                => {
                    h.add_edge(parent, child, ());
                }
                Discover(_, _) | Finish(_, _)  => {}
            }
        });
        let sorted = toposort(&h, None).map_err(|_e| GraphError::Cycle)?;
        let mut sorted_ancestors = sorted.into_iter();
        
        Ok(sorted_ancestors
            .into_iter()
            .map(|i| g.graph[i].clone())
            .collect())
    }

    pub fn get_dependent_nodes(&self, refn: &str) -> Result<Vec<Node>, GraphError> {
        let g = self.graph.lock().unwrap();
        let start_node = g.get_index(refn)?;
        let mut h = DiGraphMap::new();
        depth_first_search(&g.graph, Some(start_node), |event| {
            use petgraph::visit::DfsEvent::*;
            match event {
                CrossForwardEdge(parent, child)
                | BackEdge(parent, child)
                | TreeEdge(parent, child) => {
                    h.add_edge(parent, child, ());
                }
                Discover(_, _) | Finish(_, _) => {}
            }
        });
        let sorted = toposort(&h, None).map_err(|_e| GraphError::Cycle)?;
        Ok(sorted
            .into_iter()
            .rev()
            .map(|i| g.graph[i].clone())
            .collect())
    }
}

impl References for MutableGraph {
    fn find(&self, refn: &str) -> Option<String> {
        let g = self.graph.lock().unwrap();
        g.find(refn)
    }

    fn save(&mut self, refn: &str, value: String) {
        let mut g = self.graph.lock().unwrap();
        g.save(refn, value)
    }
}

impl oca_rs::facade::build::References for DependencyGraph {
    fn find(&self, refn: &str) -> Option<String> {
        self.get_said(refn).map(|said| said.to_string()).ok()
    }

    fn save(&mut self, refn: &str, value: String) {
        self.update_said(refn, value.parse().unwrap()).unwrap();
    }
}

#[test]
fn test_ancestors() -> anyhow::Result<()> {
    use std::{fs::File, io::Write};
    use tempdir::TempDir;

    let tmp_dir = TempDir::new("example")?;

    let first_ocafile_str = "-- name=first\nADD ATTRIBUTE d=Text i=Text passed=Boolean";
    let second_ocafile_str = "-- name=second\nADD ATTRIBUTE list=Array[Text] el=Text";
    let third_ocafile_str = "-- name=third\nADD ATTRIBUTE first=refn:first second=refn:second";
    let fourth_ocafile_str =
        "-- name=fourth\nADD ATTRIBUTE whatever=Text";
    let fifth_ocafile_str =
    "-- name=fifth\nADD ATTRIBUTE third=refn:third four=refn:fourth";

    let list = [
        ("first.ocafile", first_ocafile_str),
        ("second.ocafile", second_ocafile_str),
        ("third.ocafile", third_ocafile_str),
        ("fourth.ocafile", fourth_ocafile_str),
        ("fifth.ocafile", fifth_ocafile_str),
    ];

    let mut paths = vec![];
    for (name, contents) in list {
        let path = tmp_dir.path().join(name);
        let mut tmp_file = File::create(&path)?;
        writeln!(tmp_file, "{}", contents)?;
        paths.push(path)
    }

    let petgraph = MutableGraph::new(tmp_dir.path(), paths);

    let first_anc = petgraph.get_ancestors("first").unwrap().into_iter().map(|node| node.refn).collect::<Vec<_>>();
    assert_eq!(first_anc, vec!["third", "fifth"]);
    
    let fourth_anc = petgraph.get_ancestors("fourth").unwrap().into_iter().map(|node| node.refn).collect::<Vec<_>>();
    assert_eq!(fourth_anc, vec!["fifth"]);

    Ok(())
}
