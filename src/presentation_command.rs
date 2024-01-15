use std::{path::PathBuf, fs::{self, File}, io::Write, str::FromStr, collections::BTreeMap};

use isolang::Language;
use oca_presentation::{presentation::{Presentation, self}, page::{Page, PageElement}};
use said::{sad::SAD, SelfAddressingIdentifier};

use crate::get_oca_facade;

pub fn handle_parse(from_file: &PathBuf, output: &Option<PathBuf>) {
	// load file
	let file = fs::read_to_string(from_file).expect("Should have been able to read the file");
	// deserialize presentation
	let mut pres: Presentation = serde_json::from_str(&file).unwrap();
	// compute digest and insert in `d`
	pres.compute_digest();
	// save to file
	let out_path = if let Some(out) = output {out} else {from_file};
	let mut file = File::create(out_path).unwrap();
	file.write_all(serde_json::to_string_pretty(&pres).unwrap().as_bytes()).unwrap();
}

pub fn handle_get(said: SelfAddressingIdentifier, local_repository_path: PathBuf) {
	let facade = get_oca_facade(local_repository_path);
	match facade.get_oca_bundle(said, true) {
		Ok(oca_bundles) => {
			let bundle = oca_bundles.bundle;
			let attributes = bundle.capture_base.attributes.clone();

			for (name, attr) in attributes {
				println!("{}: {:?}", name, attr);
			}

			let page = Page { name: "main".to_string(), attribute_order: vec![PageElement::Value("attr_1".to_string())] };

			let mut pages_label = BTreeMap::new();
			let mut pages_label_en = BTreeMap::new();
			pages_label_en.insert("pageY".to_string(), "Page Y".to_string());
			pages_label_en.insert("pageZ".to_string(), "Page Z".to_string());
			// Generate for all available languages
			pages_label.insert(Language::Eng, pages_label_en);


			let mut presentation_base = presentation::Presentation {
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
					}]
			};
			presentation_base.compute_digest();

			println!(
				"{}",
				serde_json::to_string_pretty(&presentation_base).unwrap()
			);

		},
		Err(errors) => {
			println!("{:?}", errors);
		}
	}
}