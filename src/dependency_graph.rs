use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use petgraph::{algo::toposort, graph::NodeIndex, Graph};
use regex::Regex;
use said::SelfAddressingIdentifier;

#[derive(Default, Debug, Clone)]
pub struct Node {
    pub refn: String,
    pub path: PathBuf,
}

pub struct DependencyGraph {
    graph: Graph<Node, ()>,
}

impl DependencyGraph {
    pub fn new<I, P>(file_paths: I) -> Self
    where
        P: AsRef<Path>,
        I: IntoIterator<Item = P>,
    {
        // Helper variable for saving connections between nodes, before it was
        // added to graph. Key is refn, and values are indices of nodes, that
        // should have connection with node of given refn.
        let mut edges_to_save = HashMap::new();
        let mut out = DependencyGraph {
            graph: Graph::<Node, ()>::new(),
        };
        file_paths
            .into_iter()
            .map(|path| Self::parse_oca_file(&path.as_ref()))
            .for_each(|(node, dependencies)| {
                let index = out.insert_node(node, &mut edges_to_save);
                for dep in dependencies {
                    let edges = edges_to_save.get_mut(&dep);
                    match edges {
                        Some(edges) => {
                            edges.push(index.clone());
                        }
                        None => {
                            edges_to_save.insert(dep.clone(), vec![index]);
                        }
                    };
                }
            });

        // Process remaining edges.
        for (refn, nodes) in edges_to_save.iter() {
            let ind = out.get_index(&refn).unwrap();
            let edges = nodes.into_iter().map(|n| (n.to_owned(), ind));
            out.graph.extend_with_edges(edges);
        }
        out
    }

    pub fn sort(&self) -> Vec<Node> {
        let sorted = toposort(&self.graph, None).unwrap();
        sorted
            .into_iter()
            .rev()
            .map(|i| self.graph[i].clone())
            .collect()
    }

    pub fn get_index(&self, refn: &str) -> Option<NodeIndex> {
        self.graph
            .node_indices()
            .find(|id| self.graph[id.clone()].refn.eq(&refn))
    }

    pub fn neighbors(&self, refn: &str) -> Vec<Node> {
        let index = self.get_index(refn).unwrap();
        self.graph
            .neighbors(index)
            .map(|n| self.graph[n].clone())
            .collect::<Vec<_>>()
    }

    pub fn oca_file_path(&self, refn: &str) -> Option<PathBuf> {
        let index = self.get_index(refn).unwrap();
        Some(self.graph[index].path.clone())
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
        match edges_to_save.remove(&refn) {
            Some(edges) => {
                for edge in edges {
                    self.graph.add_edge(edge, index, ());
                }
            }
            None => (),
        }
        index
    }

    fn parse_oca_file(file_path: &Path) -> (Node, Vec<String>) {
        let content = fs::read_to_string(file_path).expect("Failed to read file");
        let lines: Vec<&str> = content.lines().collect();
        let ref_name_line = lines.first().expect("File is empty");
        match ref_name_line.split("name=").nth(1) {
            Some(name_part) => {
                let ref_name = name_part.trim_matches('"').to_string();
                let ref_node = Node {
                    refn: ref_name,
                    path: file_path.into(),
                };
                (ref_node, Self::find_refn(lines))
            }
            None => {
                print!("RefN not found in parsed file: {:?}", file_path);
                // None
                todo!()
            }
        }
    }

    fn find_refn(lines: Vec<&str>) -> Vec<String> {
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

#[test]
fn test_sort() -> anyhow::Result<()> {
    use std::{fs::File, io::Write};
    use tempdir::TempDir;

    let tmp_dir = TempDir::new("example")?;

    let first_ocafile_str = "-- name=first\nADD ATTRIBUTE d=Text i=Text passed=Boolean";
    let second_ocafile_str = "-- name=second\nADD ATTRIBUTE list=Array[Text] el=Text";
    let third_ocafile_str = "-- name=third\nADD ATTRIBUTE first=refn:first second=refn:second";
    let fourth_ocafile_str =
        "-- name=fourth\nADD ATTRIBUTE first=refn:first second=refn:second third=refn:third";

    let list = [
        ("first.ocafile", first_ocafile_str),
        ("second.ocafile", second_ocafile_str),
        ("third.ocafile", third_ocafile_str),
        ("fourth.ocafile", fourth_ocafile_str),
    ];

    let mut paths = vec![];
    for (name, contents) in list {
        let path = tmp_dir.path().join(name);
        let mut tmp_file = File::create(&path)?;
        writeln!(tmp_file, "{}", contents)?;
        paths.push(path)
    }

    let petgraph = DependencyGraph::new(paths);
    assert_eq!(
        petgraph
            .sort()
            .iter()
            .map(|node| node.refn.clone())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third", "fourth"]
    );

    let n: Vec<_> = petgraph
        .neighbors("fourth")
        .iter()
        .map(|n| n.refn.clone())
        .collect();
    assert_eq!(n.len(), 3);
    assert!(n.contains(&"first".into()));
    assert!(n.contains(&"second".into()));
    assert!(n.contains(&"third".into()));

    Ok(())
}
