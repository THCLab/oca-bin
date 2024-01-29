use clap::Subcommand;
use indexmap::IndexMap;
use isolang::Language;
use itertools::Itertools;
use oca_ast::ast::recursive_attributes::NestedAttrTypeFrame;
use oca_ast::ast::{AttributeType, NestedAttrType, OverlayType, RefValue};
use oca_bundle::state::oca::OCABundle;
use oca_presentation::page::recursion_setup::PageElementFrame;
use oca_presentation::presentation::AttrType;
use oca_presentation::{
    page::{Page, PageElement},
    presentation::{self, Presentation},
};
use oca_rs::Facade;
use recursion::{CollapsibleExt, ExpandableExt};
use said::{sad::SAD, SelfAddressingIdentifier};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

#[derive(Subcommand)]
pub enum PresentationCommand {
    /// Generate presentation for OCA bundle of provided SAID
    Generate {
        /// SAID of OCA Bundle
        #[arg(short, long)]
        said: String,
        /// Presentation output format: json or yaml. Default is json
        #[arg(short, long)]
        format: Option<Format>,
    },
    /// Parse presentation from file and validate its SAID. To recalculate it's
    /// digest use `-r` flag.
    Validate {
        /// Path to input file
        #[arg(short, long)]
        from_file: PathBuf,
        /// Path to output file
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Presentation output format: json or yaml. Default is json
        #[arg(long)]
        format: Option<Format>,
        /// Recalculate SAID. It computes presentation SAID and put it into `d`
        /// field
        #[arg(long, short, default_value_t = false)]
        recalculate: bool,
    },
}

#[derive(Clone, Debug)]
pub enum Format {
    JSON,
    YAML,
}

impl Format {
    pub fn format<S: Serialize>(&self, data: &S) -> String {
        match self {
            Format::JSON => serde_json::to_string_pretty(data).unwrap(),
            Format::YAML => serde_yaml::to_string(data).unwrap(),
        }
    }
}

impl FromStr for Format {
    type Err = super::CliError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "json" => Ok(Self::JSON),
            "yaml" => Ok(Self::YAML),
            other => Err(super::CliError::FormatError(other.to_string())),
        }
    }
}

pub fn handle_validate(
    input_str: &str,
    format: Format,
    recalculate: bool,
) -> Result<Presentation, PresentationError> {
    let mut pres: Presentation = match format {
        Format::JSON => serde_json::from_str(input_str)?,
        Format::YAML => serde_yaml::from_str(input_str)?,
    };
    match pres.validate_digest() {
        Err(e) => {
            if recalculate {
                println!("Computing presentation digest and inserting it into `d` field.");
                pres.compute_digest();
                Ok(pres)
            } else {
                Err(e.into())
            }
        }
        Ok(_) => Ok(pres),
    }
}

pub fn handle_generate(
    said: SelfAddressingIdentifier,
    facade: &Facade,
) -> Result<Presentation, PresentationError> {
    let oca_bundles = facade
        .get_oca_bundle(said, true)
        .map_err(PresentationError::OcaBundleErrors)?;
    let dependencies = oca_bundles.dependencies;
    let bundle = oca_bundles.bundle;
    let attributes = bundle.capture_base.attributes;

    let mut attr_order = vec![];
    let mut interactions: IndexMap<String, AttrType> = IndexMap::new();
    for (name, attr) in attributes {
        let mut reference_name: Option<String> = None;
        // Convert NestedAttrType to PageElement
        let page_element = PageElement::expand_frames((name, attr), |(name, attr)| match attr {
            NestedAttrType::Array(arr) => {
                reference_name = match &reference_name {
                    Some(nested) => Some([nested, ".", &name].concat()),
                    None => Some(name.to_string()),
                };
                // Array elements can have nested references inside
                arr.collapse_frames(|frame| match frame {
                    NestedAttrTypeFrame::Reference(RefValue::Said(said)) => {
                        let more_nested_attributes = handle_reference(said.clone(), &dependencies);
                        PageElementFrame::Page {
                            name: name.clone(),
                            attribute_order: more_nested_attributes.unwrap(),
                        }
                    }
                    NestedAttrTypeFrame::Value(value) => {
                        save_interaction(
                            &name,
                            value,
                            reference_name.as_deref(),
                            &mut interactions,
                        );
                        PageElementFrame::Value(name.clone())
                    }
                    NestedAttrTypeFrame::Null => PageElementFrame::Value(name.clone()),
                    NestedAttrTypeFrame::Array(arr) => arr,
                    NestedAttrTypeFrame::Reference(RefValue::Name(_name)) => todo!(),
                })
            }
            NestedAttrType::Value(value) => {
                save_interaction(&name, value, reference_name.as_deref(), &mut interactions);
                PageElementFrame::Value(name)
            }
            NestedAttrType::Null => PageElementFrame::Value(name),
            NestedAttrType::Reference(RefValue::Said(said)) => {
                let more_nested_attributes = handle_reference(said, &dependencies);
                reference_name = match &reference_name {
                    Some(nested) => Some([nested, ".", &name].concat()),
                    None => Some(name.to_string()),
                };
                PageElementFrame::Page {
                    name,
                    attribute_order: more_nested_attributes.unwrap(),
                }
            }
            NestedAttrType::Reference(RefValue::Name(_name)) => todo!(),
        });

        attr_order.push(page_element);
    }

    let languages: Vec<_> = bundle
        .overlays
        .clone()
        .into_iter()
        .filter_map(|overlay| {
            if overlay.overlay_type() == &OverlayType::Label {
                overlay.language().copied()
            } else {
                None
            }
        })
        .unique()
        .collect();

    let page_name = "page 1".to_string();
    let mut page_translation = IndexMap::new();
    let mut eng_translation = BTreeMap::new();
    eng_translation.insert(page_name.clone(), "Page 1".to_string());
    page_translation.insert(Language::Eng, eng_translation);
    let page = Page {
        name: page_name.clone(),
        attribute_order: attr_order,
    };

    let presentation_base = presentation::Presentation {
        version: "1.0.0".to_string(),
        bundle_digest: bundle.said.clone().unwrap(),
        said: None,
        pages: vec![page],
        pages_order: vec!["page1".to_string()],
        pages_label: page_translation,
        interaction: vec![presentation::Interaction {
            interaction_method: presentation::InteractionMethod::Web,
            context: presentation::Context::Capture,
            attr_properties: interactions,
        }],
        languages,
    };

    Ok(presentation_base)
}

fn save_interaction(
    name: &str,
    value: AttributeType,
    nested: Option<&str>,
    interactions: &mut IndexMap<String, AttrType>,
) {
    let name = match &nested {
        Some(nested) => [nested, ".", name].concat(),
        None => name.to_string(),
    };
    match value {
        AttributeType::Binary => {
            interactions.insert(name.to_owned(), AttrType::File);
        }
        AttributeType::DateTime => {
            interactions.insert(name.to_owned(), AttrType::DateTime);
        }
        _ => (),
    };
}

fn handle_reference(
    said: SelfAddressingIdentifier,
    bundles: &[OCABundle],
) -> Result<Vec<(String, NestedAttrType)>, PresentationError> {
    let dependency_attrs = bundles
        .iter()
        .find(|dep| dep.said.as_ref() == Some(&said))
        .ok_or(PresentationError::MissingDependency(said))?
        .capture_base
        .attributes
        .clone();
    Ok(dependency_attrs.into_iter().collect())
}

#[derive(Debug, Error)]
pub enum PresentationError {
    #[error("Invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Invalid yaml: {0}")]
    InvalidYaml(#[from] serde_yaml::Error),
    #[error("Oca bundle errors: {0:?}")]
    OcaBundleErrors(Vec<String>),
    #[error("Missing dependency to oca bundle of said {0}")]
    MissingDependency(SelfAddressingIdentifier),
    #[error(transparent)]
    Presentation(#[from] presentation::PresentationError),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use isolang::Language;
    use oca_presentation::{page::PageElement, presentation::AttrType};

    use crate::{get_oca_facade, presentation_command::handle_generate};

    #[test]
    fn test_handle_references() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file0 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

        // Value oca bundle
        let oca_bundle0 = facade.build_from_ocafile(oca_file0).unwrap();
        let digest0 = oca_bundle0.said.unwrap();

        let oca_file1 = format!(
            "ADD ATTRIBUTE person=refs:{}\nADD ATTRIBUTE like_cats=Boolean",
            digest0.to_string()
        );

        // Reference oca bundle
        let oca_bundle1 = facade.build_from_ocafile(oca_file1).unwrap();
        let digest1 = oca_bundle1.said.unwrap();

        let presentation = handle_generate(digest1.clone(), &facade).unwrap();

        let page_element_1 = PageElement::Value("like_cats".to_string());
        let page_element_2 = PageElement::Page {
            name: "person".to_string(),
            attribute_order: vec![
                PageElement::Value("name".to_string()),
                PageElement::Value("number".to_string()),
            ],
        };

        assert_eq!(
            presentation.pages.get(0).unwrap().attribute_order,
            vec![page_element_1, page_element_2.clone()]
        );

        dbg!(presentation);

        let oca_file2 = format!(
            "ADD ATTRIBUTE cat_lover=refs:{}\nADD ATTRIBUTE favorite_cat=Text",
            digest1.to_string()
        );

        // Reference to Reference oca bundle
        let oca_bundle2 = facade.build_from_ocafile(oca_file2).unwrap();
        let digest2 = oca_bundle2.said.unwrap();

        let presentation = handle_generate(digest2.clone(), &facade).unwrap();

        let page_element_3 = PageElement::Page {
            name: "cat_lover".to_string(),
            attribute_order: vec![
                PageElement::Value("like_cats".to_string()),
                PageElement::Page {
                    name: "person".to_string(),
                    attribute_order: vec![
                        PageElement::Value("name".to_string()),
                        PageElement::Value("number".to_string()),
                    ],
                },
            ],
        };
        let page_element_4 = PageElement::Value("favorite_cat".to_string());

        assert_eq!(
            presentation.pages.get(0).unwrap().attribute_order,
            vec![page_element_3.clone(), page_element_4.clone()]
        );
    }

    #[test]
    fn test_handle_array() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        // Array of values
        let oca_file0 = "ADD ATTRIBUTE list=Array[Numeric] name=Text".to_string();

        // Reference oca bundle
        let array_bundle = facade.build_from_ocafile(oca_file0.clone()).unwrap();
        let array_bundle_said = array_bundle.said.unwrap();

        let presentation = handle_generate(array_bundle_said.clone(), &facade).unwrap();

        let expected_presentation_json = r#"{"v":"1.0.0","bd":"EJi486RStLv0EzSOaOfY1RtCPfY7-tGBdS6CnFLacKqW","l":[],"d":"","p":[{"n":"page 1","ao":["list","name"]}],"po":["page1"],"pl":{"eng":{"page 1":"Page 1"}},"i":[{"m":"web","c":"capture","a":{}}]}"#;
        assert_eq!(
            expected_presentation_json,
            serde_json::to_string(&presentation).unwrap()
        );

        let person_page_element = vec![
            PageElement::Value("list".to_string()),
            PageElement::Value("name".to_string()),
        ];

        assert_eq!(
            presentation.pages.get(0).unwrap().attribute_order,
            person_page_element.clone()
        );

        let oca_file1 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

        // Value oca bundle
        let oca_bundle0 = facade.build_from_ocafile(oca_file1.clone()).unwrap();
        let digest0 = oca_bundle0.said.unwrap();

        let presentation = handle_generate(digest0.clone(), &facade).unwrap();

        let expected_presentation_json = r#"{"v":"1.0.0","bd":"EEx1y3CnK5LcByLUb_MF7hR3Iv-Fs8enGdbYCiiil21T","l":[],"d":"","p":[{"n":"page 1","ao":["name","number"]}],"po":["page1"],"pl":{"eng":{"page 1":"Page 1"}},"i":[{"m":"web","c":"capture","a":{}}]}"#;
        assert_eq!(
            expected_presentation_json,
            serde_json::to_string(&presentation).unwrap()
        );

        dbg!(presentation);

        let oca_file1 = format!("ADD ATTRIBUTE person=refs:{}", digest0.to_string());

        // Reference oca bundle
        let person_oca_bundle = facade.build_from_ocafile(oca_file1.clone()).unwrap();
        let person_bundle_said = person_oca_bundle.said.unwrap();

        let presentation = handle_generate(person_bundle_said.clone(), &facade).unwrap();

        let expected_presentation_json = r#"{"v":"1.0.0","bd":"EGU0faBu85GSuo4rwDAo7Qi52OpZpHS8GutS8Rh5rIfl","l":[],"d":"","p":[{"n":"page 1","ao":[{"n":"person","ao":["name","number"]}]}],"po":["page1"],"pl":{"eng":{"page 1":"Page 1"}},"i":[{"m":"web","c":"capture","a":{}}]}"#;
        assert_eq!(
            expected_presentation_json,
            serde_json::to_string(&presentation).unwrap()
        );

        let person_page_element = PageElement::Page {
            name: "person".to_string(),
            attribute_order: vec![
                PageElement::Value("name".to_string()),
                PageElement::Value("number".to_string()),
            ],
        };

        assert_eq!(
            presentation.pages.get(0).unwrap().attribute_order,
            vec![person_page_element.clone()]
        );

        dbg!(presentation);

        // Array of references oca bundle
        let oca_file2 = format!(
            "ADD ATTRIBUTE many_persons=Array[refs:{}]",
            person_bundle_said.to_string()
        );

        let many_persons_bundle = facade.build_from_ocafile(oca_file2.clone()).unwrap();
        let many_person_bundle_digest = many_persons_bundle.said.unwrap();

        let presentation = handle_generate(many_person_bundle_digest, &facade).unwrap();

        let expected_presentation_json = r#"{"v":"1.0.0","bd":"EDqTtz-Lp5tWstJ8nLfhpe5UC1cnFQkA27CZQeSfnvHs","l":[],"d":"","p":[{"n":"page 1","ao":[{"n":"many_persons","ao":[{"n":"person","ao":["name","number"]}]}]}],"po":["page1"],"pl":{"eng":{"page 1":"Page 1"}},"i":[{"m":"web","c":"capture","a":{}}]}"#;
        assert_eq!(
            expected_presentation_json,
            serde_json::to_string(&presentation).unwrap()
        );

        let page_element_5 = PageElement::Page {
            name: "many_persons".to_string(),
            attribute_order: vec![person_page_element],
        };

        assert_eq!(
            presentation.pages.get(0).unwrap().attribute_order,
            vec![page_element_5]
        );
    }

    #[test]
    fn test_languages() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file = r#"ADD ATTRIBUTE name=Text age=Numeric radio=Text
ADD LABEL eo ATTRS name="Nomo" age="aĝo" radio="radio"
ADD LABEL pl ATTRS name="Imię" age="wiek" radio="radio"
ADD INFORMATION en ATTRS name="Object" age="Object"
ADD CHARACTER_ENCODING ATTRS name="utf-8" age="utf-8"
ADD ENTRY_CODE ATTRS radio=["o1", "o2", "o3"]
ADD ENTRY eo ATTRS radio={"o1": "etikedo1", "o2": "etikedo2", "o3": "etikiedo3"}
ADD ENTRY pl ATTRS radio={"o1": "etykieta1", "o2": "etykieta2", "o3": "etykieta3"}
"#;

        let oca_bundle = facade.build_from_ocafile(oca_file.to_string()).unwrap();
        let digest = oca_bundle.said.unwrap();

        let presentation = handle_generate(digest, &facade).unwrap();
        assert_eq!(presentation.languages, vec![Language::Epo, Language::Pol]);
        let translations = &presentation.pages_label;
        let eng_expected: BTreeMap<String, String> =
            serde_json::from_str(r#"{"page 1": "Page 1"}"#).unwrap();
        assert_eq!(translations.get(&Language::Eng).unwrap(), &eng_expected);

        println!("{}", serde_json::to_string_pretty(&presentation).unwrap());
    }

    #[test]
    fn test_interaction() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file = r#"ADD ATTRIBUTE radio=Text dt=DateTime img=Binary"#;

        let oca_bundle = facade.build_from_ocafile(oca_file.to_string()).unwrap();
        let digest = oca_bundle.said.unwrap();

        let presentation = handle_generate(digest, &facade).unwrap();
        let interaction_attrs = presentation.interaction[0].clone().attr_properties;
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("dt").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::DateTime).unwrap()
        );
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("img").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::File).unwrap()
        );

        println!("{}", serde_json::to_string_pretty(&presentation).unwrap());
    }

    #[test]
    fn test_complex_interaction() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file = r#"ADD ATTRIBUTE radio=Text dt=DateTime img=Binary"#;

        let oca_bundle = facade.build_from_ocafile(oca_file.to_string()).unwrap();
        let digest = oca_bundle.said.unwrap();

        let oca_file_2 = format!(r#"ADD ATTRIBUTE nested=refs:{}"#, digest.to_string());
        let oca_bundle2 = facade.build_from_ocafile(oca_file_2.to_string()).unwrap();
        let nested_digest = oca_bundle2.said.unwrap();

        let oca_file_3 = format!(
            r#"ADD ATTRIBUTE again=refs:{} once=refs:{}"#,
            nested_digest.to_string(),
            digest.to_string()
        );
        let oca_bundle3 = facade.build_from_ocafile(oca_file_3.to_string()).unwrap();
        let nested_digest = oca_bundle3.said.unwrap();

        let presentation = handle_generate(nested_digest, &facade).unwrap();
        let interaction_attrs = presentation.interaction[0].clone().attr_properties;
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("once.dt").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::DateTime).unwrap()
        );
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("once.img").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::File).unwrap()
        );
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("again.nested.dt").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::DateTime).unwrap()
        );
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("again.nested.img").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::File).unwrap()
        );

        let oca_file_4 = format!(r#"ADD ATTRIBUTE list=Array[refs:{}]"#, digest.to_string());
        let oca_bundle4 = facade.build_from_ocafile(oca_file_4.to_string()).unwrap();
        let array_digest = oca_bundle4.said.unwrap();
        let presentation = handle_generate(array_digest, &facade).unwrap();
        let interaction_attrs = presentation.interaction[0].clone().attr_properties;
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("list.dt").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::DateTime).unwrap()
        );
        assert_eq!(
            serde_json::to_string(interaction_attrs.get("list.img").unwrap()).unwrap(),
            serde_json::to_string(&AttrType::File).unwrap()
        );

        println!("{}", serde_json::to_string_pretty(&presentation).unwrap());
    }
}
