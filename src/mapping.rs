use std::{fs::File, path::PathBuf};

use oca_ast::ast::{NestedAttrType, RefValue};
use oca_bundle::state::{attribute::Attribute, oca::OCABundle};
use oca_rs::Facade;
use said::SelfAddressingIdentifier;
use serde_json::{Map, Value};

use crate::{
    dependency_graph::DependencyGraph, error::CliError, get_oca_facade, utils::load_ocafiles_all,
};

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
    let mut map = Map::new();
    map.insert("digest".to_string(), Value::String(said.to_string()));
    bundle
        .capture_base
        .attributes
        .into_iter()
        .map(|(name, attr)| handle_attr(name, attr, facade, dep_graph))
        .flatten()
        .for_each(|key| {
            map.insert(key, Value::String("".to_string()));
        });

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
        NestedAttrType::Value(value) => {
            vec![name]
        }
        NestedAttrType::Array(_) => todo!(),
        NestedAttrType::Null => todo!(),
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

#[test]
fn test_handle_references() {
    use std::io::Write;
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
        ("first.ocafile", oca_file0),
        ("second.ocafile", oca_file1),
        ("third.ocafile", oca_file2),
    ];

    let mut paths = vec![];
    for (name, contents) in list {
        let path = tmp_dir.path().join(name);
        let mut tmp_file = File::create(&path).unwrap();
        writeln!(tmp_file, "{}", contents).unwrap();
        paths.push(path)
    }
    let dependency_graph = DependencyGraph::from_paths(tmp_dir.path(), paths).unwrap();
    let o = mapping(digest2, &facade, &dependency_graph).unwrap();

    let expected_json = r#"{
  "digest": "EJFw2ZOSK0wdiKTmB0dvovdi9Y20Xb5Aye4DuvmW_qKT",
  "cat_lover.like_cats": "",
  "cat_lover.person.name": "",
  "cat_lover.person.number": "",
  "favorite_cat": ""
}"#;

    let actual_json = serde_json::to_string_pretty(&o).unwrap();
    assert_eq!(expected_json, actual_json)
}

// #[test]
//  fn test_handle_array() {
//     let tmp_dir = tempdir::TempDir::new("db").unwrap();

//     let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

//     // Array of values
//     let oca_file0 = "ADD ATTRIBUTE list=Array[Numeric] name=Text".to_string();

//     // Reference oca bundle
//     let array_bundle = facade.build_from_ocafile(oca_file0.clone()).unwrap();
//     let array_bundle_said = array_bundle.said.unwrap();

//     let presentation = handle_flatten(array_bundle_said.clone(), &facade).unwrap();

//     let oca_file1 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

//     // Value oca bundle
//     let oca_bundle0 = facade.build_from_ocafile(oca_file1.clone()).unwrap();
//     let digest0 = oca_bundle0.said.unwrap();

//     let presentation = handle_flatten(digest0.clone(), &facade).unwrap();

//     let oca_file1 = format!("ADD ATTRIBUTE person=refs:{}", digest0.to_string());

//     // Reference oca bundle
//     let person_oca_bundle = facade.build_from_ocafile(oca_file1.clone()).unwrap();
//     let person_bundle_said = person_oca_bundle.said.unwrap();

//     let presentation = handle_flatten(person_bundle_said.clone(), &facade).unwrap();

//     // Array of references oca bundle
//     let oca_file2 = format!(
//         "ADD ATTRIBUTE many_persons=Array[refs:{}]",
//         person_bundle_said.to_string()
//     );

//     let many_persons_bundle = facade.build_from_ocafile(oca_file2.clone()).unwrap();
//     let many_person_bundle_digest = many_persons_bundle.said.unwrap();

//     let presentation = handle_flatten(many_person_bundle_digest, &facade).unwrap();

//  }
