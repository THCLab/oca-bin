use isolang::Language;
use oca_ast::ast::{NestedAttrType, RefValue};
use oca_presentation::page::recursion_setup::PageElementFrame;
use oca_presentation::{
    page::{Page, PageElement},
    presentation::{self, Presentation},
};
use oca_rs::Facade;
use recursion::ExpandableExt;
use said::{sad::SAD, SelfAddressingIdentifier};
use std::collections::BTreeMap;
use thiserror::Error;

pub fn handle_parse(input_str: &str) -> Result<Presentation, PresentationError> {
    let mut pres: Presentation = serde_json::from_str(&input_str)?;
    match pres.validate_digest() {
        Err(presentation::PresentationError::MissingSaid) => {
            pres.compute_digest();
            Ok(pres)
        }
        Err(presentation::PresentationError::SaidDoesNotMatch) => {
            Err(presentation::PresentationError::SaidDoesNotMatch.into())
        }
        Ok(_) => Ok(pres),
    }
}

pub fn handle_get(
    said: SelfAddressingIdentifier,
    facade: Facade,
) -> Result<Presentation, PresentationError> {
    let oca_bundles = facade
        .get_oca_bundle(said, true)
        .map_err(PresentationError::OcaBundleErrors)?;
    let dependencies = oca_bundles.dependencies;
    let bundle = oca_bundles.bundle;
    let attributes = bundle.capture_base.attributes;

    let mut attr_order = vec![];
    for (name, attr) in attributes {
        let page_element = PageElement::expand_frames((name, attr), |(name, attr)| match attr {
            NestedAttrType::Value(_) | NestedAttrType::Array(_) | NestedAttrType::Null => {
                PageElementFrame::Value(name)
            }
            NestedAttrType::Reference(RefValue::Said(said)) => {
                let dependency_attrs = dependencies
                    .iter()
                    .find(|dep| dep.said.as_ref() == Some(&said.clone()))
                    .expect(&format!("There's no dependency: {}", said.to_string()))
                    .capture_base.attributes
                    .clone();
                let more_nested_attributes = dependency_attrs
                    .into_iter()
                    .map(|(key, value)| (key, value))
                    .collect();
                PageElementFrame::Page {
                    name,
                    attribute_order: more_nested_attributes,
                }
            }
            NestedAttrType::Reference(RefValue::Name(_name)) => todo!(),
        });

        attr_order.push(page_element);
    }

    let page = Page {
        name: "Page 1".to_string(),
        attribute_order: attr_order,
    };

    let mut pages_label = indexmap::IndexMap::new();
    let mut pages_label_en = BTreeMap::new();
    pages_label_en.insert("page1".to_string(), "Page 1".to_string());
    // Generate for all available languages
    pages_label.insert(Language::Eng, pages_label_en);

    let mut presentation_base = presentation::Presentation {
        version: "1.0.0".to_string(),
        bundle_digest: bundle.said.clone().unwrap(),
        said: None,
        pages: vec![page],
        pages_order: vec!["page1".to_string()],
        pages_label,
        interaction: vec![presentation::Interaction {
            interaction_method: presentation::InteractionMethod::Web,
            context: presentation::Context::Capture,
            attr_properties: vec![("attr_1".to_string(), presentation::AttrType::TextArea)]
                .into_iter()
                .collect(),
        }],
        languages: vec![],
    };
    presentation_base.compute_digest();

    Ok(presentation_base)
}

#[derive(Debug, Error)]
pub enum PresentationError {
    #[error("Invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Oca bundle errors: {0:?}")]
    OcaBundleErrors(Vec<String>),
    #[error("Missing dependency to oca bundle of said {0}")]
    MissingDependency(SelfAddressingIdentifier),
    #[error(transparent)]
    Presentation(#[from] presentation::PresentationError),
}

#[cfg(test)]
mod tests {
    use oca_presentation::page::PageElement;

    use crate::{get_oca_facade, presentation_command::handle_get};

    #[test]
    fn test_handle_get() {
        let tmp_dir = tempdir::TempDir::new("db").unwrap();

        let mut facade = get_oca_facade(tmp_dir.path().to_path_buf());

        let oca_file0 = r#"ADD ATTRIBUTE name=Text number=Numeric"#.to_string();

        let oca_bundle1 = facade.build_from_ocafile(oca_file0).unwrap();
        let digest1 = oca_bundle1.said.unwrap();

        let oca_file1 = format!(
            "ADD ATTRIBUTE person=refs:{}\nADD ATTRIBUTE like_cats=Boolean",
            digest1.to_string()
        );

        let oca_bundle2 = facade.build_from_ocafile(oca_file1).unwrap();
        let digest2 = oca_bundle2.said.unwrap();

        let presentation = handle_get(digest2, facade).unwrap();

        let page_element_1 = PageElement::Value("like_cats".to_string());
        let page_element_2 = PageElement::Page {
            name: "person".to_string(),
            attribute_order: vec![
                PageElement::Value("name".to_string()),
                PageElement::Value("number".to_string()),
            ],
        };

        assert!(presentation
            .pages
            .get(0)
            .unwrap()
            .attribute_order
            .contains(&page_element_1));
        assert!(presentation
            .pages
            .get(0)
            .unwrap()
            .attribute_order
            .contains(&page_element_2));

        dbg!(presentation);
    }
}
