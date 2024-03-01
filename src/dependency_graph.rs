use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use regex::Regex;

pub fn find_refn(lines: Vec<&str>) -> Vec<String> {
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

pub fn parse_file(file_path: PathBuf) -> Option<(String, PathBuf, Vec<String>)> {
    let content = fs::read_to_string(file_path.clone()).expect("Failed to read file");
    let lines: Vec<&str> = content.lines().collect();
    let ref_name_line = lines.first().expect("File is empty");
    match ref_name_line.split("name=").nth(1) {
        Some(name_part) => {
            let ref_name = name_part.trim_matches('"').to_string();

            let dependencies = find_refn(lines);
            // println!(
            // "path {:?} RefN: {:?}, dependencies: {:?}",
            // file_path, ref_name, dependencies
            // );

            Some((ref_name, file_path, dependencies))
        }
        None => {
            print!("RefN not found in parsed file: {:?}", file_path);
            None
        }
    }
}

pub struct DependencyPathPair {
    pub path: PathBuf,
    pub dependencies: Vec<String>,
}

pub fn build_dependency_graph(file_paths: Vec<PathBuf>) -> HashMap<String, DependencyPathPair> {
    let mut graph = HashMap::new();
    for file_path in file_paths {
        match parse_file(file_path.clone()) {
            Some((ref_name, path, dependencies)) => {
                graph.insert(ref_name, DependencyPathPair { path, dependencies });
            }
            None => {
                println!("Failed to parse file: {:?}", file_path);
            }
        }
    }
    graph
}

pub fn topological_sort(graph: &HashMap<String, DependencyPathPair>) -> Vec<String> {
    let mut sorted = Vec::new();
    let mut visited = HashSet::new();
    let mut temp_marks = HashSet::new();
    let mut has_cycles = false;

    fn dfs(
        node: &String,
        graph: &HashMap<String, DependencyPathPair>,
        visited: &mut HashSet<String>,
        temp_marks: &mut HashSet<String>,
        sorted: &mut Vec<String>,
        has_cycles: &mut bool,
    ) {
        if visited.contains(node) {
            return;
        }
        if temp_marks.contains(node) {
            *has_cycles = true; // Cycle detected
            return;
        }

        temp_marks.insert(node.clone());

        // println!("Visiting: {}", node);
        if let Some(dep_pair) = graph.get(node) {
            let mut dependencies = dep_pair.dependencies.clone();
            dependencies.sort(); // Ensure deterministic order
            for dep in dependencies {
                dfs(&dep, graph, visited, temp_marks, sorted, has_cycles);
            }
        }

        temp_marks.remove(node);
        visited.insert(node.clone());
        // println!("Adding to sorted: {}", node);
        sorted.push(node.clone());
    }

    let mut keys: Vec<_> = graph.keys().cloned().collect();
    keys.sort(); // Ensure deterministic order
    for node in keys {
        dfs(
            &node,
            graph,
            &mut visited,
            &mut temp_marks,
            &mut sorted,
            &mut has_cycles,
        );
    }

    if has_cycles {
        // Handle cycle detection case
        println!("Warning: Cycles detected in the graph");
    }
    sorted
}
