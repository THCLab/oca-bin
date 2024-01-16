use std::{collections::BTreeMap, path::PathBuf};

use isolang::Language;
use oca_presentation::{
    page::{Page, PageElement},
    presentation::{self, Presentation},
};
use said::{sad::SAD, SelfAddressingIdentifier};
use thiserror::Error;

use crate::get_oca_facade;

pub fn handle_parse(input_str: &str) -> Result<Presentation, PresentationError> {
    let mut pres: Presentation = serde_json::from_str(&input_str)?;
    match pres.validate_digest() {
        Err(presentation::PresentationError::MissingSaid) => {
            pres.compute_digest();
            Ok(pres)
        },
        Err(presentation::PresentationError::SaidDoesNotMatch) => Err(presentation::PresentationError::SaidDoesNotMatch.into()),
        Ok(_) => Ok(pres),
    }
}

pub fn handle_get(
    said: SelfAddressingIdentifier,
    local_repository_path: PathBuf,
) -> Result<(), PresentationError> {
    let facade = get_oca_facade(local_repository_path);
    let oca_bundles = facade
        .get_oca_bundle(said, true)
        .map_err(PresentationError::OcaBundleErrors)?;
    let bundle = oca_bundles.bundle;
    let attributes = bundle.capture_base.attributes.clone();

    for (name, attr) in attributes {
        println!("{}: {:?}", name, attr);
    }

    let page = Page {
        name: "main".to_string(),
        attribute_order: vec![PageElement::Value("attr_1".to_string())],
    };

    let mut pages_label = BTreeMap::new();
    let mut pages_label_en = BTreeMap::new();
    pages_label_en.insert("pageY".to_string(), "Page Y".to_string());
    pages_label_en.insert("pageZ".to_string(), "Page Z".to_string());
    // Generate for all available languages
    pages_label.insert(Language::Eng, pages_label_en);

    let mut presentation_base = presentation::Presentation {
        version: "1.0.0".to_string(),
        bundle_digest: bundle.said.clone().unwrap(),
        said: None,
        pages: vec![page],
        pages_order: vec!["pageY".to_string(), "pageZ".to_string()],
        pages_label,
        interaction: vec![presentation::Interaction {
            interaction_method: presentation::InteractionMethod::Web,
            context: presentation::Context::Capture,
            attr_properties: vec![(
                "attr_1".to_string(),
                presentation::Properties {
                    type_: presentation::AttrType::TextArea,
                },
            )]
            .into_iter()
            .collect(),
        }],
    };
    presentation_base.compute_digest();

    println!(
        "{}",
        serde_json::to_string_pretty(&presentation_base).unwrap()
    );
    Ok(())
}

#[derive(Debug, Error)]
pub enum PresentationError {
    #[error("Invalid json: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Oca bundle errors: {0:?}")]
    OcaBundleErrors(Vec<String>),
    #[error(transparent)]
    Presentation(#[from] presentation::PresentationError)
}
