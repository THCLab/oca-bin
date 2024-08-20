use oca_ast::ast::{NestedAttrType, RefValue};
use oca_rs::Facade;
use said::SelfAddressingIdentifier;
use serde_json::{Map, Value};

use crate::{dependency_graph::DependencyGraph, error::CliError};

/// Generates json with all attributes of OCA element of given SAID
pub fn mapping(
    said: SelfAddressingIdentifier,
    facade: &Facade,
    dep_graph: &DependencyGraph,
) -> Result<Map<String, Value>, CliError> {
    let oca_bundles = facade
        .get_oca_bundle(said.clone(), true)
        .map_err(CliError::OcaBundleAstError)?;
    let bundle = oca_bundles.bundle;
    let capture_base_said = bundle.capture_base.said.clone().unwrap();
    let mut map = Map::new();
    map.insert(
        "capture_base".to_string(),
        Value::String(capture_base_said.to_string()),
    );
    let mut attribute_mapping = Map::new();
    bundle
        .capture_base
        .attributes
        .into_iter()
        .map(|(name, attr)| handle_attr(name, attr, facade, dep_graph))
        .flatten()
        .for_each(|key| {
            attribute_mapping.insert(key, Value::String("".to_string()));
        });
    map.insert(
        "attribute_mapping".to_string(),
        Value::Object(attribute_mapping),
    );

    Ok(map)
}

fn handle_attr(
    name: String,
    attr: NestedAttrType,
    facade: &Facade,
    dep_graph: &DependencyGraph,
) -> Vec<String> {
    match attr {
        NestedAttrType::Reference(RefValue::Name(name)) => {
            let said = dep_graph.get_said(&name).unwrap();
            handle_reference(said, &name, facade, dep_graph)
        }
        NestedAttrType::Reference(RefValue::Said(said)) => {
            handle_reference(said, &name, facade, dep_graph)
        }
        NestedAttrType::Value(_) => {
            vec![name]
        }
        NestedAttrType::Array(attr) => handle_attr(name, *attr, facade, dep_graph),
        NestedAttrType::Null => vec![name],
    }
}

fn handle_reference(
    said: SelfAddressingIdentifier,
    name: &str,
    facade: &Facade,
    dep_graph: &DependencyGraph,
) -> Vec<String> {
    let oca_bundles = facade.get_oca_bundle(said, true).unwrap();
    let bundle = oca_bundles.bundle;
    let attributes = bundle.capture_base.attributes;

    attributes
        .into_iter()
        .map(|(inside_name, attr)| {
            handle_attr(inside_name.clone(), attr, &facade, dep_graph)
                .iter()
                .map(|attribute| [name, ".", &attribute].concat())
                .collect::<Vec<_>>()
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{dependency_graph::DependencyGraph, get_oca_facade, mapping::mapping};
    use std::{fs::File, io::Write};

    #[test]
    fn test_handle_references() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file0 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

        // Value oca bundle
        let oca_bundle0 = facade.build_from_ocafile(oca_file0.clone()).unwrap();
        let digest0 = oca_bundle0.said.unwrap();

        let oca_file1 = format!(
            "ADD ATTRIBUTE person=refs:{}\nADD ATTRIBUTE like_cats=Boolean",
            digest0.to_string()
        );

        // Reference oca bundle
        let oca_bundle1 = facade.build_from_ocafile(oca_file1.clone()).unwrap();
        let digest1 = oca_bundle1.said.unwrap();

        let oca_file2 = format!(
            "ADD ATTRIBUTE cat_lover=refs:{}\nADD ATTRIBUTE favorite_cat=Text",
            digest1.to_string()
        );

        // Reference to Reference oca bundle
        let oca_bundle2 = facade.build_from_ocafile(oca_file2.clone()).unwrap();
        let digest2 = oca_bundle2.said.unwrap();

        // Build temporary directory with test ocafiles.
        let list = [
            ("first", oca_file0),
            ("second", oca_file1),
            ("third", oca_file2),
        ];

        let mut paths = vec![];
        for (name, contents) in list {
            let path = tmp_dir.path().join(format!("{}.ocafile", name));
            let mut tmp_file = File::create(&path).unwrap();
            writeln!(tmp_file, "-- name={}", name).unwrap();

            paths.push(path)
        }
        let dependency_graph = DependencyGraph::from_paths(tmp_dir.path(), paths).unwrap();
        let o = mapping(digest2, &facade, &dependency_graph).unwrap();

        let expected_json = r#"{
  "capture_base": "EAF0irS_GXSZaAlb99_zv7LJ-9I1Ljdv6RvVvPPZFrQb",
  "attribute_mapping": {
    "cat_lover.like_cats": "",
    "cat_lover.person.name": "",
    "cat_lover.person.number": "",
    "favorite_cat": ""
  }
}"#;

        let actual_json = serde_json::to_string_pretty(&o).unwrap();
        assert_eq!(expected_json, actual_json)
    }

    #[test]
    fn test_handle_array() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file1 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

        // Value oca bundle
        let oca_bundle0 = facade.build_from_ocafile(oca_file1.clone()).unwrap();
        let digest0 = oca_bundle0.said.unwrap();

        let oca_file2 = format!("ADD ATTRIBUTE person=refs:{}", digest0.to_string());

        // Reference oca bundle
        let person_oca_bundle = facade.build_from_ocafile(oca_file2.clone()).unwrap();
        let person_bundle_said = person_oca_bundle.said.unwrap();

        // Array of references oca bundle
        let oca_file3 = format!(
            "ADD ATTRIBUTE many_persons=Array[refs:{}]",
            person_bundle_said.to_string()
        );

        let many_persons_bundle = facade.build_from_ocafile(oca_file3.clone()).unwrap();
        let many_person_bundle_digest = many_persons_bundle.said.unwrap();

        // Build temporary directory with test ocafiles.
        let list = [
            ("first", oca_file1),
            ("second", oca_file2),
            ("third", oca_file3),
        ];
        let mut paths = vec![];
        for (name, contents) in list {
            let path = tmp_dir.path().join(format!("{}.ocafile", name));
            let mut tmp_file = File::create(&path).unwrap();
            writeln!(tmp_file, "-- name={}", name).unwrap();
            writeln!(tmp_file, "{}", contents).unwrap();
            paths.push(path)
        }

        let dependency_graph = DependencyGraph::from_paths(tmp_dir.path(), paths).unwrap();
        let o = mapping(
            many_person_bundle_digest.clone(),
            &facade,
            &dependency_graph,
        )
        .unwrap();

        let expected_json = r#"{
  "capture_base": "ECR3Kq3QmrVJYwu2ibLPScAU3mlZQs7H1o3nBJYho5vU",
  "attribute_mapping": {
    "many_persons.person.name": "",
    "many_persons.person.number": ""
  }
}"#;
        let actual_json = serde_json::to_string_pretty(&o).unwrap();
        assert_eq!(expected_json, actual_json);
    }
}
